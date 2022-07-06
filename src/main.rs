use mlua::Lua;
use nix::{
    ioctl_none_bad, ioctl_write_int_bad,
    libc::{
        self, fd_set, input_event, timeval, ABS_MAX, FD_ISSET, FD_SET, KEY_MAX, O_NONBLOCK,
        O_RDONLY, O_WRONLY, REL_MAX,
    },
    request_code_none, request_code_write,
};
use std::{
    cell::{Cell, RefCell},
    ffi::CString,
    os::unix::prelude::OsStrExt,
    path::PathBuf,
    process::Stdio,
    rc::Rc,
    thread::JoinHandle,
};
use structopt::StructOpt;

const EV_SYN: i32 = 0;
const EV_KEY: i32 = 1;
const EV_REL: i32 = 2;
const EV_MSC: i32 = 4;
const BUS_USB: u16 = 3;

ioctl_write_int_bad!(eviocgrab, request_code_write!('E', 0x90, 4));
ioctl_write_int_bad!(ui_set_evbit, request_code_write!('U', 100, 4));
ioctl_write_int_bad!(ui_set_keybit, request_code_write!('U', 101, 4));
ioctl_write_int_bad!(ui_set_relbit, request_code_write!('U', 102, 4));
ioctl_none_bad!(ui_dev_create, request_code_none!('U', 1));

#[derive(structopt::StructOpt)]
#[structopt(name = "shortcuts")]
struct Args {
    script: PathBuf,
    devices: Vec<PathBuf>,
    #[structopt(long, short)]
    name: String,
}

fn main() {
    let args = Args::from_args();
    unsafe {
        match run(&args.devices, &args.name, &args.script) {
            Ok(()) => {}
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
    }
}

unsafe fn run(
    devices: &[PathBuf],
    device_name: &str,
    script: impl AsRef<std::path::Path>,
) -> Result<(), mlua::Error> {
    let devices: Vec<i32> = open_devices(devices);
    let fd = create_uinput(device_name);
    loop {
        run_event_loop(&devices, &script, fd)?
    }
}

unsafe fn run_event_loop(
    devices: &[i32],
    script: impl AsRef<std::path::Path>,
    fd: i32,
) -> Result<(), mlua::Error> {
    let children = Rc::new(RefCell::new(Vec::new()));
    let lua = create_lua(children.clone());
    let script = std::fs::read_to_string(&script).unwrap();
    lua_attach_send_event(&lua, fd);
    lua.load(&script).exec()?;
    let event_callback: mlua::Function = lua.globals().get("on_event")?;
    let exec_callback: mlua::Function = lua.globals().get("exec_callback")?;
    let should_break = Rc::new(Cell::new(false));
    lua.globals().set(
        "reload",
        lua.create_function({
            let should_break = should_break.clone();
            move |_, ()| {
                should_break.set(true);
                Ok(())
            }
        })?,
    )?;
    let mut ev: input_event = input_event {
        time: timeval {
            tv_sec: 0,
            tv_usec: 0,
        },
        type_: 0,
        code: 0,
        value: 0,
    };
    loop {
        if should_break.get() {
            for (handle, _) in children.borrow_mut().drain(..) {
                handle.join().unwrap();
            }
            break;
        }
        if !children.borrow().is_empty() {
            {
                let mut children = children.borrow_mut();
                let finished = children
                    .iter()
                    .enumerate()
                    .find(|(_, (handle, _))| handle.is_finished())
                    .map(|(i, _)| i);
                if let Some(index) = finished {
                    let (handle, ident) = children.swap_remove(index);
                    // The lua script could call `execute` in the `exec_callback`.
                    // If the `children` wasn't dropped then the implementation
                    // of `execute` would try to borrow children again causing a
                    // BorrowMutError from RefCell.
                    drop(children);
                    let output = handle.join().unwrap();
                    let output = lua.create_string(&output)?;
                    exec_callback.call((ident, output))?;
                }
            }
        }
        let mut set: fd_set = std::mem::transmute([0u8; std::mem::size_of::<fd_set>()]);
        for fd in devices {
            FD_SET(*fd, &mut set as *mut fd_set);
        }
        let mut timeout: timeval = timeval {
            tv_sec: 0,
            tv_usec: 500_000, // 500 ms
        };
        let code = libc::select(
            devices.iter().copied().max().unwrap_or(0) + 1,
            &mut set as *mut fd_set,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut timeout as *mut timeval,
        );
        if code == 0 {
            continue;
        }
        for (device, fd) in devices.iter().copied().enumerate() {
            if !FD_ISSET(fd, &set as *const fd_set) {
                continue;
            }
            let code = libc::read(
                fd,
                &mut ev as *mut _ as *mut libc::c_void,
                std::mem::size_of_val(&ev),
            );
            if code < 0 {
                Result::<(), std::io::Error>::Err(std::io::Error::last_os_error()).unwrap();
            }
            if code < 24 {
                continue;
            }
            event_callback
                .call::<(usize, u16, u16, i32), ()>((device, ev.type_, ev.code, ev.value))?;
        }
    }
    Ok(())
}

fn open_devices(devices: &[PathBuf]) -> Vec<i32> {
    devices
        .iter()
        .map(|path| unsafe {
            let path = CString::new(path.as_path().as_os_str().as_bytes()).unwrap();
            let fdi = libc::open(path.as_ptr(), O_RDONLY);
            if fdi < 0 {
                eprintln!("cannot device {:?}", path);
                Result::<(), std::io::Error>::Err(std::io::Error::last_os_error()).unwrap();
            }
            // grab all input from the input device
            eviocgrab(fdi, 1).unwrap();
            fdi
        })
        .collect()
}

unsafe fn create_uinput(device_name: &str) -> i32 {
    let path = CString::new("/dev/uinput").unwrap();
    let fdo = libc::open(path.as_ptr(), O_WRONLY | O_NONBLOCK);
    if fdo < 0 {
        println!("cannot open /dev/uinput");
        Result::<(), std::io::Error>::Err(std::io::Error::last_os_error()).unwrap();
    }

    ui_set_evbit(fdo, EV_SYN).unwrap();
    ui_set_evbit(fdo, EV_KEY).unwrap();
    ui_set_evbit(fdo, EV_REL).unwrap();
    ui_set_evbit(fdo, EV_MSC).unwrap();

    for i in 0..KEY_MAX as i32 {
        ui_set_keybit(fdo, i).unwrap();
    }
    for i in 0..REL_MAX as i32 {
        ui_set_relbit(fdo, i).unwrap();
    }

    if device_name.is_empty() {
        eprintln!("name cannot be empty");
        std::process::exit(1);
    }
    let mut name = [0; 0x50];
    for (i, b) in device_name.as_bytes().iter().enumerate() {
        name[i] = *b as i8;
    }
    let uidev = libc::uinput_user_dev {
        name,
        id: libc::input_id {
            bustype: BUS_USB,
            vendor: 1,
            product: 1,
            version: 1,
        },
        ff_effects_max: 0,
        absmax: [0; ABS_MAX as usize + 1],
        absmin: [0; ABS_MAX as usize + 1],
        absfuzz: [0; ABS_MAX as usize + 1],
        absflat: [0; ABS_MAX as usize + 1],
    };
    if libc::write(
        fdo,
        &uidev as *const _ as *const libc::c_void,
        std::mem::size_of_val(&uidev),
    ) < 0
    {
        Result::<(), std::io::Error>::Err(std::io::Error::last_os_error()).unwrap();
    }

    ui_dev_create(fdo).unwrap();
    fdo
}

type Children = Rc<RefCell<Vec<(JoinHandle<Vec<u8>>, i32)>>>;
fn create_lua(children: Children) -> Lua {
    let lua = mlua::Lua::new();
    let execute = lua
        .create_function(move |_, (ident, cmd): (i32, Vec<String>)| {
            let handle = std::thread::spawn(move || {
                if let Some(program) = cmd.get(0) {
                    let output = std::process::Command::new(program)
                        .args(cmd.into_iter().skip(1))
                        .stdout(Stdio::piped())
                        .output()
                        .unwrap();
                    output.stdout
                } else {
                    Vec::new()
                }
            });
            children.borrow_mut().push((handle, ident));
            Ok(())
        })
        .unwrap();
    lua.globals().set("native_execute", execute).unwrap();
    lua
}

fn lua_attach_send_event(lua: &Lua, fd: i32) {
    lua.globals()
        .set(
            "send_event",
            lua.create_function(create_event_sender(fd)).unwrap(),
        )
        .unwrap();
}

fn create_event_sender(fd: i32) -> impl Fn(&mlua::Lua, (u16, u16, i32)) -> Result<(), mlua::Error> {
    move |_, (ty, code, value)| unsafe {
        let ev = input_event {
            time: timeval {
                tv_sec: 0,
                tv_usec: 0,
            },
            type_: ty,
            code,
            value,
        };
        if libc::write(
            fd,
            &ev as *const _ as *const libc::c_void,
            std::mem::size_of_val(&ev),
        ) < 0
        {
            Result::<(), std::io::Error>::Err(std::io::Error::last_os_error()).unwrap();
        }
        Ok(())
    }
}
