use mlua::Lua;
use nix::{
    ioctl_none_bad, ioctl_write_int_bad, ioctl_write_ptr_bad,
    libc::{
        self, fd_set, input_event, timeval, FD_ISSET, FD_SET, O_NONBLOCK, O_RDONLY, O_WRONLY,
        REL_MAX,
    },
    request_code_none, request_code_write,
};
use std::{
    cell::Cell,
    collections::HashMap,
    ffi::CString,
    io::{Read, Write},
    os::unix::prelude::OsStrExt,
    path::PathBuf,
    process::Stdio,
    rc::Rc,
    sync::{mpsc::Sender, Arc, Mutex},
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
    // Wait for user to release key.
    std::thread::sleep(std::time::Duration::from_millis(200));
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
            Ok(true) => continue,
            Ok(false) => {
                destroy_uinput(fd)?;
                break Ok(());
            }
            Err(e) => {
                destroy_uinput(fd)?;
                break Err(e);
            }
        }
    }
}

unsafe fn run_event_loop(
    devices: &[i32],
    script: impl AsRef<std::path::Path>,
    fd: i32,
) -> Result<bool, Error> {
    let (tx, rx) = std::sync::mpsc::channel();
    let should_exit = Rc::new(Cell::new(false));
    let children_stdins = Arc::new(Mutex::new(HashMap::new()));
    let lua = create_lua(tx, should_exit.clone(), children_stdins)?;
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
        if should_exit.get() {
            break Ok(false);
        }
        if should_break.get() {
            break Ok(true);
        }
        const EXEC_CALLBACK_EXIT: i32 = 0;
        const EXEC_CALLBACK_STDOUT: i32 = 1;
        const EXEC_CALLBACK_STDERR: i32 = 2;
        for (message, ident) in rx.try_iter() {
            match message {
                ProcessMessage::Stdout(data) => {
                    let data = lua.create_string(&data)?;
                    exec_callback.call((ident, EXEC_CALLBACK_STDOUT, data))?;
                }
                ProcessMessage::Stderr(data) => {
                    let data = lua.create_string(&data)?;
                    exec_callback.call((ident, EXEC_CALLBACK_STDERR, data))?;
                }
                ProcessMessage::Exit(code) => {
                    exec_callback.call((ident, EXEC_CALLBACK_EXIT, code))?;
                }
            }
        }
        let mut set: fd_set = std::mem::transmute([0u8; std::mem::size_of::<fd_set>()]);
        for fd in devices {
            FD_SET(*fd, &mut set as *mut fd_set);
        }
        let mut timeout: timeval = timeval {
            tv_sec: 0,
            tv_usec: 200_000, // 100 ms
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

enum ProcessMessage {
    Stdout(Vec<u8>),
    Stderr(Vec<u8>),
    Exit(i32),
}

fn create_lua(
    tx: Sender<(ProcessMessage, i32)>,
    should_exit: Rc<Cell<bool>>,
    children_stdins: Arc<Mutex<HashMap<i32, Sender<Vec<u8>>>>>,
) -> Result<Lua, Error> {
    let lua = mlua::Lua::new();
    let execute = lua
        .create_function({
            let children_stdins = children_stdins.clone();
            move |_, (ident, cmd, args): (i32, String, Vec<String>)| {
                let (stdin_tx, stdin_rx) = std::sync::mpsc::channel();
                children_stdins.lock().unwrap().insert(ident, stdin_tx);
                std::thread::spawn({
                    let tx = tx.clone();
                    let children_stdins = Arc::clone(&children_stdins);
                    move || {
                        let child = std::process::Command::new(cmd)
                            .args(args.into_iter())
                            .stdin(Stdio::piped())
                            .stdout(Stdio::piped())
                            .stderr(Stdio::piped())
                            .spawn();
                        let mut child = match child {
                            Ok(child) => child,
                            Err(error) => {
                                tx.send((ProcessMessage::Stderr(format!("{error}").into()), ident))
                                    .ok();
                                tx.send((
                                    ProcessMessage::Exit(error.raw_os_error().unwrap_or(1)),
                                    ident,
                                ))
                                .ok();
                                children_stdins.lock().unwrap().remove(&ident);
                                return;
                            }
                        };
                        let mut stdin = child.stdin.take().unwrap();
                        let mut stdout = child.stdout.take().unwrap();
                        let mut stderr = child.stderr.take().unwrap();
                        std::thread::spawn(move || {
                            for data in stdin_rx {
                                match stdin.write_all(&data).and_then(|_| stdin.flush()) {
                                    Ok(()) => {}
                                    Err(_) => break,
                                }
                            }
                        });
                        std::thread::spawn({
                            let tx = tx.clone();
                            move || {
                                let mut buf = [0; 128];
                                loop {
                                    let read = match stdout.read(&mut buf) {
                                        Ok(read) => read,
                                        Err(e) => match e.kind() {
                                            std::io::ErrorKind::Interrupted => continue,
                                            _ => break,
                                        },
                                    };
                                    if read == 0 {
                                        std::thread::sleep(std::time::Duration::from_millis(200));
                                        continue;
                                    }
                                    if tx
                                        .send((
                                            ProcessMessage::Stdout(buf[0..read].to_vec()),
                                            ident,
                                        ))
                                        .is_err()
                                    {
                                        break;
                                    }
                                }
                                drop(stdout);
                            }
                        });
                        std::thread::spawn({
                            let tx = tx.clone();
                            move || loop {
                                let mut buf = [0; 128];
                                let read = match stderr.read(&mut buf) {
                                    Ok(read) => read,
                                    Err(e) => match e.kind() {
                                        std::io::ErrorKind::Interrupted => continue,
                                        _ => break,
                                    },
                                };
                                if read == 0 {
                                    std::thread::sleep(std::time::Duration::from_millis(200));
                                    continue;
                                }
                                if tx
                                    .send((ProcessMessage::Stderr(buf[0..read].to_vec()), ident))
                                    .is_err()
                                {
                                    break;
                                }
                            }
                        });
                        let exit_code = child.wait().unwrap().code().unwrap();
                        tx.send((ProcessMessage::Exit(exit_code), ident)).ok();
                        children_stdins.lock().unwrap().remove(&ident);
                    }
                });
                Ok(())
            }
        })
        .unwrap();
    let write_stdin = lua
        .create_function(move |_, (ident, value): (i32, mlua::String<'_>)| {
            Ok(match children_stdins.lock().unwrap().get(&ident) {
                Some(sender) => sender.send(value.as_bytes().to_vec()).is_ok(),
                None => false,
            })
        })
        .unwrap();
    lua.globals().set("__async_execute", execute)?;
    lua.globals().set("__process_write_stdin", write_stdin)?;
    lua.globals().set(
        "__exit",
        lua.create_function({
            move |_, ()| {
                should_exit.set(true);
                Ok(())
            }
        })?,
    )?;
    Ok(lua)
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
