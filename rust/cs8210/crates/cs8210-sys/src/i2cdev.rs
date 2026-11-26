//! i2c-dev `SMBus` byte transfers (`i2cget` / `i2cset` with a byte register), for the
//! LED and system CPLDs on the `SoC`'s I2C mux tree. The kernel's mux drivers select the
//! channel per transfer and serialise the bus with every other user (fand, lm75).
#![allow(unsafe_code)]

use std::fs::{File, OpenOptions};
use std::io;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};

// <linux/i2c-dev.h>, <linux/i2c.h>
const I2C_SLAVE: libc::c_ulong = 0x0703;
const I2C_SMBUS: libc::c_ulong = 0x0720;
const I2C_SMBUS_READ: u8 = 1;
const I2C_SMBUS_WRITE: u8 = 0;
const I2C_SMBUS_BYTE_DATA: u32 = 2;
const I2C_SMBUS_WORD_DATA: u32 = 3;
const I2C_SMBUS_BLOCK_DATA: u32 = 5;

/// `union i2c_smbus_data` is 34 bytes; the byte member is at offset 0.
type SmbusData = [u8; 34];

/// `struct i2c_smbus_ioctl_data`.
#[repr(C)]
struct SmbusIoctlData {
    read_write: u8,
    command: u8,
    size: u32,
    data: *mut SmbusData,
}

/// An open `/dev/i2c-N`.
#[derive(Debug)]
pub struct I2cDev {
    file: File,
    path: PathBuf,
}

impl I2cDev {
    /// Open an adapter's character device.
    pub fn open(path: &Path) -> io::Result<Self> {
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        Ok(Self {
            file,
            path: path.to_path_buf(),
        })
    }

    /// The device path (for messages).
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn claim(&self, addr: u8) -> io::Result<()> {
        // I2C_SLAVE, not _FORCE: an address a kernel driver owns is refused (EBUSY).
        // SAFETY: an integer argument; the descriptor is open for the call.
        if unsafe { libc::ioctl(self.file.as_raw_fd(), I2C_SLAVE as _, libc::c_ulong::from(addr)) } < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    fn smbus(&self, write: bool, reg: u8, data: &mut SmbusData) -> io::Result<()> {
        self.smbus_sized(write, reg, I2C_SMBUS_BYTE_DATA, data)
    }

    fn smbus_sized(&self, write: bool, reg: u8, size: u32, data: &mut SmbusData) -> io::Result<()> {
        let mut args = SmbusIoctlData {
            read_write: if write { I2C_SMBUS_WRITE } else { I2C_SMBUS_READ },
            command: reg,
            size,
            data: std::ptr::from_mut(data),
        };
        // SAFETY: `args` has the kernel's #[repr(C)] layout and points at a live 34-byte
        // buffer (the union's size); both outlive the call; the kernel writes at most
        // those 34 bytes.
        if unsafe { libc::ioctl(self.file.as_raw_fd(), I2C_SMBUS as _, std::ptr::from_mut(&mut args)) } < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// `SMBus` read byte data: register `reg` of the 7-bit address `addr`.
    pub fn read_byte_data(&self, addr: u8, reg: u8) -> io::Result<u8> {
        self.claim(addr)?;
        let mut data: SmbusData = [0; 34];
        self.smbus(false, reg, &mut data)?;
        Ok(data[0])
    }

    /// `SMBus` read word data (little-endian on the wire, as `PMBus` sends it).
    pub fn read_word_data(&self, addr: u8, reg: u8) -> io::Result<u16> {
        self.claim(addr)?;
        let mut data: SmbusData = [0; 34];
        self.smbus_sized(false, reg, I2C_SMBUS_WORD_DATA, &mut data)?;
        Ok(u16::from_le_bytes([data[0], data[1]]))
    }

    /// `SMBus` block read (byte count first, at most 32 bytes).
    pub fn read_block_data(&self, addr: u8, reg: u8) -> io::Result<Vec<u8>> {
        self.claim(addr)?;
        let mut data: SmbusData = [0; 34];
        self.smbus_sized(false, reg, I2C_SMBUS_BLOCK_DATA, &mut data)?;
        let n = usize::from(data[0]).min(32);
        Ok(data.get(1..=n).map(<[u8]>::to_vec).unwrap_or_default())
    }

    /// `SMBus` write byte data.
    pub fn write_byte_data(&self, addr: u8, reg: u8, value: u8) -> io::Result<()> {
        self.claim(addr)?;
        let mut data: SmbusData = [0; 34];
        data[0] = value;
        self.smbus(true, reg, &mut data)
    }
}
