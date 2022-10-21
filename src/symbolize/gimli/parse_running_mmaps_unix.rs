// Note: This file is only currently used on targets that call out to the code
// in `mod libs_dl_iterate_phdr` (e.g. linux, freebsd, ...); it may be more
// general purpose, but it hasn't been tested elsewhere.

use super::mystd::io::BufRead;
use super::{OsString, Vec};

#[derive(PartialEq, Eq, Debug)]
pub(super) struct MapsEntry {
    /// start (inclusive) and limit (exclusive) of address range.
    address: (usize, usize),
    /// The perms field are the permissions for the entry
    ///
    /// r = read
    /// w = write
    /// x = execute
    /// s = shared
    /// p = private (copy on write)
    perms: [char; 4],
    /// Offset into the file (or "whatever").
    offset: usize,
    /// device (major, minor)
    dev: (usize, usize),
    /// inode on the device. 0 indicates that no inode is associated with the memory region (e.g. uninitalized data aka BSS).
    inode: usize,
    /// Usually the file backing the mapping.
    ///
    /// Note: The man page for proc includes a note about "coordination" by
    /// using readelf to see the Offset field in ELF program headers. pnkfelix
    /// is not yet sure if that is intended to be a comment on pathname, or what
    /// form/purpose such coordination is meant to have.
    ///
    /// There are also some pseudo-paths:
    /// "[stack]": The initial process's (aka main thread's) stack.
    /// "[stack:<tid>]": a specific thread's stack. (This was only present for a limited range of Linux verisons; it was determined to be too expensive to provide.)
    /// "[vdso]": Virtual dynamically linked shared object
    /// "[heap]": The process's heap
    ///
    /// The pathname can be blank, which means it is an anonymous mapping
    /// obtained via mmap.
    ///
    /// Newlines in pathname are replaced with an octal escape sequence.
    ///
    /// The pathname may have "(deleted)" appended onto it if the file-backed
    /// path has been deleted.
    ///
    /// Note that modifications like the latter two indicated above imply that
    /// in general the pathname may be ambiguous. (I.e. you cannot tell if the
    /// denoted filename actually ended with the text "(deleted)", or if that
    /// was added by the maps rendering.
    pathname: OsString,
}

pub(super) fn parse_maps() -> Result<Vec<MapsEntry>, &'static str> {
    let mut v = Vec::new();
    let proc_self_maps = std::fs::File::open("/proc/self/maps").map_err(|_| "couldnt open /proc/self/maps")?;
    let proc_self_maps = std::io::BufReader::new(proc_self_maps);
    for line in proc_self_maps.lines() {
        let line = line.map_err(|_io_error| "couldnt read line from /proc/self/maps")?;
        v.push(line.parse()?);
    }

    Ok(v)
}

impl MapsEntry {
    pub(super) fn pathname(&self) -> &OsString {
        &self.pathname
    }

    pub(super) fn ip_matches(&self, ip: usize) -> bool {
        self.address.0 <= ip && ip < self.address.1
    }
}

impl std::str::FromStr for MapsEntry {
    type Err = &'static str;

    // Format: address perms offset dev inode pathname
    // e.g.: "ffffffffff600000-ffffffffff601000 --xp 00000000 00:00 0                  [vsyscall]"
    // e.g.: "7f5985f46000-7f5985f48000 rw-p 00039000 103:06 76021795                  /usr/lib/x86_64-linux-gnu/ld-linux-x86-64.so.2"
    // e.g.: "35b1a21000-35b1a22000 rw-p 00000000 00:00 0"
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts = s
            .split(' ') // space-separated fields
            .filter(|s| s.len() > 0); // multiple spaces implies empty strings that need to be skipped.
        let range_str = parts.next().ok_or("Couldn't find address")?;
        let perms_str = parts.next().ok_or("Couldn't find permissions")?;
        let offset_str = parts.next().ok_or("Couldn't find offset")?;
        let dev_str = parts.next().ok_or("Couldn't find dev")?;
        let inode_str = parts.next().ok_or("Couldn't find inode")?;
        let pathname_str = parts.next().unwrap_or(""); // pathname may be omitted.

        let hex = |s| usize::from_str_radix(s, 16).map_err(|_| "couldnt parse hex number");
        let address = {
            let (start, limit) = range_str.split_once('-').ok_or("Couldn't parse address range")?;
            (hex(start)?, hex(limit)?)
        };
        let perms: [char; 4] = {
            let mut chars = perms_str.chars();
            let mut c = || chars.next().ok_or("insufficient perms");
            let perms = [c()?, c()?, c()?, c()?];
            if chars.next().is_some() { return Err("too many perms"); }
            perms
        };
        let offset = hex(offset_str)?;
        let dev = {
            let (major, minor) = dev_str.split_once(':').ok_or("Couldn't parse dev")?;
            (hex(major)?, hex(minor)?)
        };
        let inode = hex(inode_str)?;
        let pathname = pathname_str.into();

        Ok(MapsEntry { address, perms, offset, dev, inode, pathname })
    }
}

#[test]
fn check_maps_entry_parsing() {
    assert_eq!("ffffffffff600000-ffffffffff601000 --xp 00000000 00:00 0                  \
                [vsyscall]".parse::<MapsEntry>().unwrap(),
               MapsEntry {
                   address: (0xffffffffff600000, 0xffffffffff601000),
                   perms: ['-','-','x','p'],
                   offset: 0x00000000,
                   dev: (0x00, 0x00),
                   inode: 0x0,
                   pathname: "[vsyscall]".into(),
               });

    assert_eq!("7f5985f46000-7f5985f48000 rw-p 00039000 103:06 76021795                  \
                /usr/lib/x86_64-linux-gnu/ld-linux-x86-64.so.2".parse::<MapsEntry>().unwrap(),
                 MapsEntry {
                     address: (0x7f5985f46000, 0x7f5985f48000),
                     perms: ['r','w','-','p'],
                     offset: 0x00039000,
                     dev: (0x103, 0x06),
                     inode: 0x76021795,
                     pathname: "/usr/lib/x86_64-linux-gnu/ld-linux-x86-64.so.2".into(),
                 });
    assert_eq!("35b1a21000-35b1a22000 rw-p 00000000 00:00 0".parse::<MapsEntry>().unwrap(),
                 MapsEntry {
                     address: (0x35b1a21000, 0x35b1a22000),
                     perms: ['r','w','-','p'],
                     offset: 0x00000000,
                     dev: (0x00,0x00),
                     inode: 0x0,
                     pathname: Default::default(),
                 });
}
