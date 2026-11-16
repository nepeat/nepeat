//! Opening a serial port: exclusive, raw, non-blocking, line settings from the config.

use crate::config::{Flow, Parity, Port};
use std::ffi::CString;
use std::io;
use std::os::fd::{FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;

pub fn baud_constant(baud: u32) -> Option<libc::speed_t> {
    Some(match baud {
        1200 => libc::B1200,
        2400 => libc::B2400,
        4800 => libc::B4800,
        9600 => libc::B9600,
        19200 => libc::B19200,
        38400 => libc::B38400,
        57600 => libc::B57600,
        115200 => libc::B115200,
        230400 => libc::B230400,
        460800 => libc::B460800,
        921600 => libc::B921600,
        1000000 => libc::B1000000,
        2000000 => libc::B2000000,
        _ => return None,
    })
}

fn check(r: libc::c_int) -> io::Result<()> {
    if r < 0 { Err(io::Error::last_os_error()) } else { Ok(()) }
}

/// Open `port.device` exclusively (TIOCEXCL: a second opener gets EBUSY) in raw mode.
pub fn open(port: &Port) -> io::Result<OwnedFd> {
    let path = CString::new(port.device.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NUL in device path"))?;
    let raw = unsafe {
        libc::open(path.as_ptr(), libc::O_RDWR | libc::O_NOCTTY | libc::O_NONBLOCK | libc::O_CLOEXEC)
    };
    if raw < 0 {
        return Err(io::Error::last_os_error());
    }
    let fd = unsafe { OwnedFd::from_raw_fd(raw) };
    configure(raw, port)?;
    Ok(fd)
}

fn configure(fd: libc::c_int, port: &Port) -> io::Result<()> {
    unsafe {
        check(libc::ioctl(fd, libc::TIOCEXCL))?;
        let mut t: libc::termios = std::mem::zeroed();
        check(libc::tcgetattr(fd, &mut t))?;
        libc::cfmakeraw(&mut t);
        t.c_cflag |= libc::CLOCAL | libc::CREAD;
        t.c_cflag &= !(libc::CSIZE | libc::PARENB | libc::PARODD | libc::CSTOPB | libc::CRTSCTS);
        t.c_cflag |= match port.data_bits {
            5 => libc::CS5,
            6 => libc::CS6,
            7 => libc::CS7,
            _ => libc::CS8,
        };
        match port.parity {
            Parity::None => {}
            Parity::Even => t.c_cflag |= libc::PARENB,
            Parity::Odd => t.c_cflag |= libc::PARENB | libc::PARODD,
        }
        if port.stop_bits == 2 {
            t.c_cflag |= libc::CSTOPB;
        }
        t.c_iflag &= !(libc::IXON | libc::IXOFF | libc::IXANY);
        match port.flow {
            Flow::None => {}
            Flow::Rtscts => t.c_cflag |= libc::CRTSCTS,
            Flow::Xonxoff => t.c_iflag |= libc::IXON | libc::IXOFF,
        }
        t.c_cc[libc::VMIN] = 0;
        t.c_cc[libc::VTIME] = 0;
        let speed = baud_constant(port.baud).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "baud"))?;
        check(libc::cfsetispeed(&mut t, speed))?;
        check(libc::cfsetospeed(&mut t, speed))?;
        check(libc::tcsetattr(fd, libc::TCSANOW, &t))?;
    }
    Ok(())
}

pub fn send_break(fd: libc::c_int) -> io::Result<()> {
    check(unsafe { libc::tcsendbreak(fd, 0) })
}
