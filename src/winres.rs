//! The Windows resource file (`.res`) that gives `mimamori.exe` its icon and
//! its version information, written byte by byte.
//!
//! Windows' `rc.exe`, which crates such as `winresource` run, comes with the
//! Windows SDK and does not exist on a Linux or macOS host. The `.res` format
//! is small, and both `link.exe` and `lld-link` take a `.res` among their
//! inputs, so `build.rs` writes one here and hands it to the linker: nothing
//! to install, and a cross build from another OS gets the icon too.
//!
//! `build.rs` includes this file as it is, so it uses nothing but `std`; the
//! tests read what it writes back.

/// The resource types used: `RT_ICON`, `RT_GROUP_ICON` and `RT_VERSION`.
const RT_ICON: u16 = 3;
const RT_GROUP_ICON: u16 = 14;
const RT_VERSION: u16 = 16;
/// US English and Unicode, as the version block's `040904B0` says.
const LANG: u16 = 0x0409;
const CODEPAGE: u16 = 1200;

/// What the version information says.
pub struct Info<'a> {
    /// Major, minor, patch.
    pub version: [u16; 3],
    /// The version as text, `0.29.0`.
    pub text: &'a str,
    /// The program's name, `mimamori`: Task Manager lists the process by it.
    pub name: &'a str,
    /// A line on what it is.
    pub comments: &'a str,
}

/// The whole `.res`: the icon as one image per size (`side` in pixels and
/// the image as PNG), and the version block.
pub fn res(icons: &[(u32, Vec<u8>)], info: &Info) -> Vec<u8> {
    let mut out = Vec::new();
    // A `.res` opens with an empty entry, which marks the format.
    entry(&mut out, 0, 0, 0, 0, &[]);
    for (i, (_, png)) in icons.iter().enumerate() {
        entry(&mut out, RT_ICON, i as u16 + 1, 0x1010, LANG, png);
    }
    entry(&mut out, RT_GROUP_ICON, 1, 0x1030, LANG, &group(icons));
    entry(&mut out, RT_VERSION, 1, 0x0030, LANG, &version(info));
    out
}

/// One entry: a 32-byte header naming the type and the ID by number, the
/// data, and padding to four bytes.
fn entry(out: &mut Vec<u8>, kind: u16, id: u16, flags: u16, lang: u16, data: &[u8]) {
    u32le(out, data.len() as u32);
    u32le(out, 32);
    for w in [0xFFFF, kind, 0xFFFF, id] {
        u16le(out, w);
    }
    u32le(out, 0); // data version
    u16le(out, flags);
    u16le(out, lang);
    u32le(out, 0); // version
    u32le(out, 0); // characteristics
    out.extend_from_slice(data);
    pad(out);
}

/// The icon group (`GRPICONDIR`): one 14-byte line per image, pointing at
/// its `RT_ICON` by ID. A side of 256 is written as 0.
fn group(icons: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut g = Vec::new();
    for w in [0, 1, icons.len() as u16] {
        u16le(&mut g, w);
    }
    for (i, (side, png)) in icons.iter().enumerate() {
        let s = if *side >= 256 { 0 } else { *side as u8 };
        g.extend_from_slice(&[s, s, 0, 0]);
        u16le(&mut g, 1); // planes
        u16le(&mut g, 32); // bits per pixel
        u32le(&mut g, png.len() as u32);
        u16le(&mut g, i as u16 + 1);
    }
    g
}

/// `VS_VERSIONINFO`: the numbers Explorer compares (`VS_FIXEDFILEINFO`) and
/// the strings it shows under Details.
fn version(info: &Info) -> Vec<u8> {
    let [major, minor, patch] = info.version.map(u32::from);
    let mut fixed = Vec::new();
    for d in [
        0xFEEF_04BD,         // signature
        0x0001_0000,         // structure version
        major << 16 | minor, // file version
        patch << 16,         //
        major << 16 | minor, // product version
        patch << 16,         //
        0x3F,                // flags that are meaningful
        0,                   // flags: a release, not a debug build
        0x0004_0004,         // VOS_NT_WINDOWS32
        1,                   // VFT_APP
        0,                   // subtype
        0,                   // date
        0,                   //
    ] {
        u32le(&mut fixed, d);
    }
    let exe = format!("{}.exe", info.name);
    let strings: Vec<Vec<u8>> = [
        ("Comments", info.comments),
        ("FileDescription", info.name),
        ("FileVersion", info.text),
        ("InternalName", info.name),
        ("OriginalFilename", &exe),
        ("ProductName", info.name),
        ("ProductVersion", info.text),
    ]
    .iter()
    .map(|(k, v)| {
        let mut w = Vec::new();
        wide(&mut w, v);
        block(k, 1, &w, (w.len() / 2) as u16, &[])
    })
    .collect();
    let table = block(&format!("{LANG:04X}{CODEPAGE:04X}"), 1, &[], 0, &strings);
    let string_info = block("StringFileInfo", 1, &[], 0, &[table]);
    let mut lang = Vec::new();
    u16le(&mut lang, LANG);
    u16le(&mut lang, CODEPAGE);
    let var = block("Translation", 0, &lang, 4, &[]);
    let var_info = block("VarFileInfo", 1, &[], 0, &[var]);
    block("VS_VERSION_INFO", 0, &fixed, fixed.len() as u16, &[string_info, var_info])
}

/// One block of the version information: its length, the value's length (in
/// bytes for binary, in characters for text), the type (0 binary, 1 text),
/// the key, the value and the children, each starting on four bytes. The
/// length leaves out the padding after the last child.
fn block(key: &str, kind: u16, value: &[u8], value_len: u16, children: &[Vec<u8>]) -> Vec<u8> {
    let mut b = vec![0, 0];
    u16le(&mut b, value_len);
    u16le(&mut b, kind);
    wide(&mut b, key);
    pad(&mut b);
    b.extend_from_slice(value);
    for c in children {
        pad(&mut b);
        b.extend_from_slice(c);
    }
    let len = b.len() as u16;
    b[..2].copy_from_slice(&len.to_le_bytes());
    b
}

/// `s` as UTF-16 with its NUL.
fn wide(out: &mut Vec<u8>, s: &str) {
    for w in s.encode_utf16().chain([0]) {
        u16le(out, w);
    }
}

fn pad(out: &mut Vec<u8>) {
    while !out.len().is_multiple_of(4) {
        out.push(0);
    }
}

fn u16le(out: &mut Vec<u8>, w: u16) {
    out.extend_from_slice(&w.to_le_bytes());
}

fn u32le(out: &mut Vec<u8>, d: u32) {
    out.extend_from_slice(&d.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u16at(b: &[u8], i: usize) -> u16 {
        u16::from_le_bytes([b[i], b[i + 1]])
    }

    fn u32at(b: &[u8], i: usize) -> u32 {
        u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
    }

    /// The entries of a `.res`: (type, ID, language, data).
    fn entries(b: &[u8]) -> Vec<(u16, u16, u16, &[u8])> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < b.len() {
            let size = u32at(b, i) as usize;
            assert_eq!(u32at(b, i + 4), 32);
            assert_eq!((u16at(b, i + 8), u16at(b, i + 12)), (0xFFFF, 0xFFFF));
            let data = &b[i + 32..i + 32 + size];
            out.push((u16at(b, i + 10), u16at(b, i + 14), u16at(b, i + 22), data));
            i += 32 + size.next_multiple_of(4);
        }
        assert_eq!(i, b.len());
        out
    }

    /// A version block: (key, value, children), checking the lengths.
    fn walk(b: &[u8]) -> (String, &[u8], Vec<&[u8]>) {
        let len = u16at(b, 0) as usize;
        let (vlen, kind) = (u16at(b, 2) as usize, u16at(b, 4));
        let mut key = Vec::new();
        let mut i = 6;
        while u16at(b, i) != 0 {
            key.push(u16at(b, i));
            i += 2;
        }
        i = (i + 2).next_multiple_of(4);
        let vbytes = if kind == 1 { vlen * 2 } else { vlen };
        let value = &b[i..i + vbytes];
        i += vbytes;
        let mut children = Vec::new();
        while i.next_multiple_of(4) < len {
            i = i.next_multiple_of(4);
            let clen = u16at(b, i) as usize;
            children.push(&b[i..i + clen]);
            i += clen;
        }
        assert_eq!(i, len);
        (String::from_utf16(&key).unwrap(), value, children)
    }

    fn text(v: &[u8]) -> String {
        let w: Vec<u16> = v.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        assert_eq!(w.last(), Some(&0));
        String::from_utf16(&w[..w.len() - 1]).unwrap()
    }

    fn sample() -> Vec<u8> {
        let icons = vec![(16, vec![1u8; 10]), (256, vec![2u8; 7])];
        let info = Info { version: [0, 29, 3], text: "0.29.3", name: "mimamori", comments: "A monitor" };
        res(&icons, &info)
    }

    #[test]
    fn the_entries_are_the_marker_two_icons_their_group_and_the_version() {
        let res = sample();
        let e = entries(&res);
        let kinds: Vec<(u16, u16)> = e.iter().map(|&(k, id, _, _)| (k, id)).collect();
        assert_eq!(kinds, [(0, 0), (RT_ICON, 1), (RT_ICON, 2), (RT_GROUP_ICON, 1), (RT_VERSION, 1)]);
        assert!(e[0].3.is_empty());
        assert_eq!(e[2].3, [2u8; 7]);
        assert!(e[1..].iter().all(|&(_, _, lang, _)| lang == LANG));

        let g = e[3].3;
        assert_eq!([u16at(g, 0), u16at(g, 2), u16at(g, 4)], [0, 1, 2]);
        // The second line: 256 written as 0, 7 bytes, ID 2.
        let line = &g[6 + 14..6 + 28];
        assert_eq!(&line[..4], [0, 0, 0, 0]);
        assert_eq!((u16at(line, 6), u32at(line, 8), u16at(line, 12)), (32, 7, 2));
        assert_eq!(g[6], 16);
    }

    #[test]
    fn the_version_block_has_the_numbers_and_the_strings() {
        let res = sample();
        let e = entries(&res);
        let (key, fixed, children) = walk(e[4].3);
        assert_eq!(key, "VS_VERSION_INFO");
        assert_eq!(fixed.len(), 52);
        assert_eq!(u32at(fixed, 0), 0xFEEF_04BD);
        assert_eq!((u32at(fixed, 8), u32at(fixed, 12)), (29, 3 << 16));

        let (key, _, tables) = walk(children[0]);
        assert_eq!(key, "StringFileInfo");
        let (key, _, strings) = walk(tables[0]);
        assert_eq!(key, "040904B0");
        let pairs: Vec<(String, String)> = strings
            .iter()
            .map(|s| {
                let (k, v, _) = walk(s);
                (k, text(v))
            })
            .collect();
        let get = |k: &str| pairs.iter().find(|(key, _)| key == k).map(|(_, v)| v.as_str());
        assert_eq!(get("FileVersion"), Some("0.29.3"));
        assert_eq!(get("FileDescription"), Some("mimamori"));
        assert_eq!(get("OriginalFilename"), Some("mimamori.exe"));
        assert_eq!(get("Comments"), Some("A monitor"));

        let (key, _, vars) = walk(children[1]);
        assert_eq!(key, "VarFileInfo");
        let (key, lang, _) = walk(vars[0]);
        assert_eq!(key, "Translation");
        assert_eq!((u16at(lang, 0), u16at(lang, 2)), (LANG, CODEPAGE));
    }

    /// What `build.rs` linked into this very test program, read back the way
    /// Explorer reads it.
    #[cfg(windows)]
    #[test]
    fn the_executable_carries_the_icon_and_the_version() {
        use windows::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW};
        use windows::Win32::System::LibraryLoader::FindResourceW;
        use windows::core::{HSTRING, PCWSTR, w};

        let exe = HSTRING::from(std::env::current_exe().unwrap().as_os_str());
        let size = unsafe { GetFileVersionInfoSizeW(&exe, None) };
        assert!(size > 0, "no version information");
        let mut data = vec![0u8; size as usize];
        unsafe { GetFileVersionInfoW(&exe, None, size, data.as_mut_ptr().cast()) }.unwrap();
        let mut p = std::ptr::null_mut();
        let mut len = 0u32;
        let ok = unsafe { VerQueryValueW(data.as_ptr().cast(), w!("\\"), &mut p, &mut len) };
        assert!(ok.as_bool() && len >= 52);
        let fixed = unsafe { std::slice::from_raw_parts(p as *const u8, len as usize) };
        let major: u32 = env!("CARGO_PKG_VERSION_MAJOR").parse().unwrap();
        let minor: u32 = env!("CARGO_PKG_VERSION_MINOR").parse().unwrap();
        assert_eq!(u32at(fixed, 8), major << 16 | minor);

        let group = unsafe {
            FindResourceW(None, PCWSTR(std::ptr::without_provenance(1)), PCWSTR(std::ptr::without_provenance(RT_GROUP_ICON as usize)))
        };
        assert!(!group.is_invalid(), "no icon group");
    }
}
