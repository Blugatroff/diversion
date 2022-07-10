# Diversion 

Diversion lets you divert raw input events like keypresses and mousemovements from the [Linux Input Subsystem](https://www.kernel.org/doc/html/v5.18/input/input_uapi.html) into a Lua script.
This Lua script can then send custom events to a [virtual input device (uinput)](https://www.kernel.org/doc/html/v5.18/input/uinput.html).

You have to manually forward an event to the virtual input device if you want the rest of your system to receive it.
That's because Diversion asked the kernel for exclusive access to the input devices so no other process on your machine will receive the events, not even X or Wayland.

## Running
```bash
cargo run --release -- --name <virual-input-device-name> <lua-script> [devices]...
```
Example:
```bash
cargo run --release -- --name "diversion" ./main.lua /dev/input/by-id/usb-NOVATEK_USB_Keyboard-event-kbd /dev/input/by-id/usb-ASUS_ROG_PUGIO-event-mouse
```
Input devices in linux are represented as files in the **/dev/input/** directory.

To let Diversion use a device you need to find the path to this file and pass it to diversion.

In the **/dev/input/by-id/** directory are symlinks to these files with helpful device names.

To test whether you found the correct device you can simply cat the file and press some buttons, if you see a bunch of binary garbage whenever you press something then you got the the right file.

## How it works
Diversion grabs exclusive access of every input device using the [EVIOCGRAB ioctl syscall](https://github.com/torvalds/linux/blob/aa051d36ce4ae23b488489f6b15abad68b59ca23/include/uapi/linux/input.h#L183).

Diversion creates a virtual input device using the [uinput kernel module](https://www.kernel.org/doc/html/v5.18/input/uinput.html) like this:
1. /dev/uinput is opened.
2. The event types EV_SYN, EV_KEY, EV_REL and EV_MSC are enabled with the [UI_SET_EVBIT](https://github.com/torvalds/linux/blob/5bfc75d92efd494db37f5c4c173d3639d4772966/include/uapi/linux/uinput.h#L137) ioctl.
3. Events for every keycode are enabled with the [UI_SET_KEYBIT](https://github.com/torvalds/linux/blob/5bfc75d92efd494db37f5c4c173d3639d4772966/include/uapi/linux/uinput.h#L138) ioctl.
4. Events for every relative movement are enabled with the [UI_SET_RELBIT](https://github.com/torvalds/linux/blob/5bfc75d92efd494db37f5c4c173d3639d4772966/include/uapi/linux/uinput.h#L139) ioctl.
5. The [UI_DEV_SETUP](https://github.com/torvalds/linux/blob/5bfc75d92efd494db37f5c4c173d3639d4772966/include/uapi/linux/uinput.h#L74) ioctl is called which passes the custom device name.
6. The [UI_DEV_CREATE](https://github.com/torvalds/linux/blob/5bfc75d92efd494db37f5c4c173d3639d4772966/include/uapi/linux/uinput.h#L64) ioctl is called.

## Interface to Lua
### Event Handler
Diversion will call the global function **<em>on_event</em>** with the **device_id**, **type**, **code** and **value** on every captured event.
- **device_id** is the index to the path of the device in the arguments passed to Diversion, e.g. <em>usb-ASUS_ROG_PUGIO-event-mouse</em> from the example would have id 1.
- **type** is the event type see: https://www.kernel.org/doc/html/v5.18/input/event-codes.html#event-types
- **code:** for keyboards this is the scancode, for mouse movements this is the axis, etc.
- **value:** for mouse movements this is the distance traveled, for keyboards this is either 1 for down 0 for up or 2 for repeated.

### Functions exposed to Lua
- **send_event**(type: number, code: number, value: number)<br>
    send an event to the virtual input device
- **reload**() -> !<br>
    reload the script, this is useful editing the script without having to restart Diversion
- **async_execute**(cmd: string, args: string[], callback: (exit_code: number, stdout: string, stderr: string) -> any)<br>
    The native [os.execute()](https://www.lua.org/pil/22.2.html) from Luas' standard library is blocking, which means that the script will block the entire main thread of Diversion and no events will be received. So if you wanted to create a shortcut to run some command then every device you passed to Diversion will be frozen whenever that command is running.

    **async_execute** avoids this by running the command and waiting for it to finish in a separate thread. Once the child process has finished the thread will exit and the **callback** will be invoked.