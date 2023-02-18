use mlua::Lua;
use nix::{
    ioctl_none_bad, ioctl_write_int_bad, ioctl_write_ptr_bad,
    libc::{
        self, fd_set, input_event, timeval, FD_ISSET, FD_SET, O_NONBLOCK, O_RDONLY,
        O_WRONLY, REL_MAX,
    },
    request_code_none, request_code_write,
};
use std::{
    cell::{Cell, RefCell},
    ffi::CString,
    os::unix::prelude::OsStrExt,
    path::PathBuf,
    process::{Output, Stdio},
    rc::Rc,
    thread::JoinHandle,
};
use structopt::StructOpt;

// https://github.com/torvalds/linux/blob/68e77ffbfd06ae3ef8f2abf1c3b971383c866983/include/uapi/linux/input-event-codes.h#L38
const EV_SYN: i32 = 0;
const EV_KEY: i32 = 1;
const EV_REL: i32 = 2;
const EV_MSC: i32 = 4;
const BUS_USB: u16 = 3;

const BTN_LEFT: i32 = 0x110;
const BTN_TASK: i32 = 0x117;

// https://github.com/torvalds/linux/blob/68e77ffbfd06ae3ef8f2abf1c3b971383c866983/include/uapi/linux/input.h#L186
ioctl_write_int_bad!(eviocgrab, request_code_write!('E', 0x90, 4));

// https://github.com/torvalds/linux/blob/5bfc75d92efd494db37f5c4c173d3639d4772966/include/uapi/linux/uinput.h#L137
ioctl_write_int_bad!(ui_set_evbit, request_code_write!('U', 100, 4));
ioctl_write_int_bad!(ui_set_keybit, request_code_write!('U', 101, 4));
ioctl_write_int_bad!(ui_set_relbit, request_code_write!('U', 102, 4));

// https://github.com/torvalds/linux/blob/5bfc75d92efd494db37f5c4c173d3639d4772966/include/uapi/linux/uinput.h#L100
ioctl_write_ptr_bad!(
    ui_dev_setup,
    request_code_write!('U', 3, std::mem::size_of::<libc::uinput_setup>()),
    libc::uinput_setup
);

// https://github.com/torvalds/linux/blob/5bfc75d92efd494db37f5c4c173d3639d4772966/include/uapi/linux/uinput.h#L64
ioctl_none_bad!(ui_dev_create, request_code_none!('U', 1));
ioctl_none_bad!(ui_dev_destroy, request_code_none!('U', 2));

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
) -> Result<(), Error> {
    let devices: Vec<i32> = open_devices(devices)?;
    let fd = create_uinput(device_name)?;
    loop {
        match run_event_loop(&devices, &script, fd) {
            Ok(()) => {},
            Err(e) => {
                destroy_uinput(fd)?;
                break Err(e);
            },
        }
    }
}

unsafe fn run_event_loop(
    devices: &[i32],
    script: impl AsRef<std::path::Path>,
    fd: i32,
) -> Result<(), Error> {
    let children = Rc::new(RefCell::new(Vec::new()));
    let lua = create_lua(children.clone());
    let script = std::fs::read_to_string(&script)?;
    lua_attach_send_event(&lua, fd);
    lua.load(&script).exec()?;
    let err_mapper = |e: mlua::Error| match e {
        mlua::Error::FromLuaConversionError { .. } => Error::from(String::from(
            "Failed to find global \"__on_event\" function!",
        )),
        e => Error::from(e),
    };
    let event_callback: mlua::Function = lua.globals().get("__on_event").map_err(err_mapper)?;
    let exec_callback: mlua::Function = lua.globals().get("__exec_callback").map_err(err_mapper)?;
    let should_break = Rc::new(Cell::new(false));
    lua.globals().set(
        "__reload",
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
    let nfds = devices.iter().copied().max().unwrap_or(0) + 1;
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
                    // The lua script could call `execute` in the `exec_callback` function.
                    // If the `children` RefCell wasn't dropped then the implementation
                    // of `execute` would try to borrow children again causing a BorrowMutError.
                    drop(children);
                    let output = handle.join().unwrap();
                    let stdout = lua.create_string(&output.stdout)?;
                    let stderr = lua.create_string(&output.stderr)?;
                    let code = output.status.code();
                    exec_callback.call((ident, code, stdout, stderr))?;
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
            nfds,
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
                Err(std::io::Error::last_os_error())?;
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

fn open_devices(devices: &[PathBuf]) -> Result<Vec<i32>, Error> {
    devices
        .iter()
        .map(|path| unsafe {
            let path_display = path.display();
            let path = CString::new(path.as_path().as_os_str().as_bytes()).unwrap();
            let fdi = libc::open(path.as_ptr(), O_RDONLY);
            if fdi < 0 {
                eprintln!("cannot access device {:?}", path);
                Result::<(), std::io::Error>::Err(std::io::Error::last_os_error())?;
            }
            // grab all input from the input device
            match eviocgrab(fdi, 1) {
                Ok(_) => {}
                Err(e) => {
                    match e {
                        nix::errno::Errno::EBUSY => {
                            eprintln!("The device {} is already busy", path_display);
                        }
                        _ => {
                            eprintln!("Failed to get exclusive access to device {}", path_display);
                        }
                    }
                    std::process::exit(1);
                }
            }
            Result::<i32, Error>::Ok(fdi)
        })
        .collect()
}

unsafe fn create_uinput(device_name: &str) -> Result<i32, Error> {
    let path = CString::new("/dev/uinput").unwrap();
    let fdo = libc::open(path.as_ptr(), O_WRONLY | O_NONBLOCK);
    if fdo < 0 {
        println!("cannot open {}", path.to_str().unwrap());
        Result::<(), std::io::Error>::Err(std::io::Error::last_os_error())?;
    }

    ui_set_evbit(fdo, EV_SYN)?;
    ui_set_evbit(fdo, EV_MSC)?;
    ui_set_evbit(fdo, EV_KEY)?;
    ui_set_evbit(fdo, EV_REL)?;

    for i in 0..255 {
        ui_set_keybit(fdo, i)?;
    }
    for i in BTN_LEFT..=BTN_TASK {
        ui_set_keybit(fdo, i)?;
    }

    //ui_set_keybit(fdo, 255)?;
    for i in 0..REL_MAX as i32 {
        ui_set_relbit(fdo, i)?;
    }
    
    ui_set_keybit(fdo, 57)?;
    if device_name.is_empty() {
        eprintln!("Name cannot be empty!");
        std::process::exit(1);
    } else if device_name.len() > 0x50 {
        eprintln!("Name cannot be longer than {} characters!", 0x50);
        std::process::exit(1);
    }
    let mut name = [0; 0x50];
    for (i, b) in device_name.as_bytes().iter().enumerate() {
        name[i] = *b as i8;
    }
    let setup: libc::uinput_setup = libc::uinput_setup {
        id: libc::input_id {
            bustype: BUS_USB,
            vendor: 1,
            product: 1,
            version: 1,
        },
        name,
        ff_effects_max: 0,
    };
    ui_dev_setup(fdo, &setup as *const libc::uinput_setup)?;
    ui_dev_create(fdo)?;
    Ok(fdo)
}

unsafe fn destroy_uinput(fd: i32) -> Result<(), Error> {
    ui_dev_destroy(fd)?;
    libc::close(fd);
    Ok(())
}

type Children = Rc<RefCell<Vec<(JoinHandle<Output>, i32)>>>;
fn create_lua(children: Children) -> Lua {
    let lua = mlua::Lua::new();
    let execute = lua
        .create_function(move |_, (ident, cmd, args): (i32, String, Vec<String>)| {
            let handle = std::thread::spawn(move || {
                std::process::Command::new(cmd)
                    .args(args.into_iter())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .output()
                    .unwrap()
            });
            children.borrow_mut().push((handle, ident));
            Ok(())
        })
        .unwrap();
    lua.globals().set("__async_execute", execute).unwrap();
    lua
}

fn lua_attach_send_event(lua: &Lua, fd: i32) {
    lua.globals()
        .set(
            "__send_event",
            lua.create_function(move |_, (ty, code, value)| unsafe {
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
            })
            .unwrap(),
        )
        .unwrap();
}

enum Error {
    Lua(mlua::Error),
    Io(std::io::Error),
    Nix(nix::errno::Errno),
    Custom(String),
}

impl From<mlua::Error> for Error {
    fn from(e: mlua::Error) -> Self {
        Self::Lua(e)
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<String> for Error {
    fn from(e: String) -> Self {
        Self::Custom(e)
    }
}

impl From<nix::errno::Errno> for Error {
    fn from(e: nix::errno::Errno) -> Self {
        Self::Nix(e)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Lua(e) => e.fmt(f),
            Error::Io(e) => e.fmt(f),
            Error::Nix(e) => e.fmt(f),
            Error::Custom(e) => e.fmt(f),
        }
    }
}
