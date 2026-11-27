//! POSIX permission and special bit representation shared by directory details and identity checks.

/// Permission bits of one owner class.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Access {
    pub read: bool,
    pub write: bool,
    pub execute: bool,
}

impl Access {
    fn from_mode(mode: u32, read: u32, write: u32, execute: u32) -> Self {
        Self {
            read: mode & read != 0,
            write: mode & write != 0,
            execute: mode & execute != 0,
        }
    }

    fn symbols(self, special: bool, special_char: char) -> [char; 3] {
        [
            if self.read { 'r' } else { '-' },
            if self.write { 'w' } else { '-' },
            match (self.execute, special) {
                (true, true) => special_char,
                (false, true) => special_char.to_ascii_uppercase(),
                (true, false) => 'x',
                (false, false) => '-',
            },
        ]
    }
}

/// Unix permission and special bits of an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Permissions {
    /// Permission and special bits only (`st_mode & 0o7777`), never the file type.
    pub mode: u32,
    pub owner: Access,
    pub group: Access,
    pub other: Access,
    pub setuid: bool,
    pub setgid: bool,
    pub sticky: bool,
}

impl Permissions {
    pub fn from_mode(mode: u32) -> Self {
        let mode = mode & 0o7777;
        Self {
            mode,
            owner: Access::from_mode(mode, 0o400, 0o200, 0o100),
            group: Access::from_mode(mode, 0o040, 0o020, 0o010),
            other: Access::from_mode(mode, 0o004, 0o002, 0o001),
            setuid: mode & 0o4000 != 0,
            setgid: mode & 0o2000 != 0,
            sticky: mode & 0o1000 != 0,
        }
    }

    /// Nine `rwx` characters. A setuid/setgid/sticky bit replaces the matching
    /// execute letter with `s`/`t`, or with `S`/`T` when execute is not set, as
    /// `ls -l` does.
    pub fn symbols(&self) -> String {
        let mut symbols = String::with_capacity(9);
        symbols.extend(self.owner.symbols(self.setuid, 's'));
        symbols.extend(self.group.symbols(self.setgid, 's'));
        symbols.extend(self.other.symbols(self.sticky, 't'));
        symbols
    }

    /// Four-digit octal form such as `0644`, `4755` or `1777`.
    pub fn octal(&self) -> String {
        format!("{:04o}", self.mode)
    }
}
