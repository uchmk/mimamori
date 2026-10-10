//! The IP helper: the interface table with each adapter's counters, the
//! unicast addresses, and the TCP and UDP tables with their owning PIDs.

use std::collections::HashMap;
use std::net::IpAddr;

use windows::Win32::NetworkManagement::IpHelper::{
    FreeMibTable, GetExtendedTcpTable, GetExtendedUdpTable, GetIfTable2, GetUnicastIpAddressTable, MIB_IF_TABLE2, MIB_TCP6TABLE_OWNER_PID,
    MIB_TCPTABLE_OWNER_PID, MIB_UDP6TABLE_OWNER_PID, MIB_UDPTABLE_OWNER_PID, MIB_UNICASTIPADDRESS_TABLE, TCP_TABLE_OWNER_PID_ALL,
    UDP_TABLE_OWNER_PID,
};
use windows::Win32::Networking::WinSock::{AF_INET, AF_INET6, AF_UNSPEC};

use super::windows::{self as fold, Row};
use super::{Conn, Conns, Nic, Note, Proto};

fn wide(s: &[u16]) -> String {
    let n = s.iter().position(|&c| c == 0).unwrap_or(s.len());
    String::from_utf16_lossy(&s[..n])
}

/// The rows of a table the IP helper allocated, freed when dropped.
struct Mib<T>(*mut T);

impl<T> Drop for Mib<T> {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: a table GetIfTable2 or GetUnicastIpAddressTable allocated, freed once.
            unsafe { FreeMibTable(self.0.cast()) };
        }
    }
}

/// Interface index to its addresses, `addr/prefix`, IPv4 and IPv6 apart.
fn addresses() -> HashMap<u32, (Vec<String>, Vec<String>)> {
    let mut out: HashMap<u32, (Vec<String>, Vec<String>)> = HashMap::new();
    let mut t = Mib::<MIB_UNICASTIPADDRESS_TABLE>(std::ptr::null_mut());
    // SAFETY: the call allocates the table; `Mib` frees it.
    if unsafe { GetUnicastIpAddressTable(AF_UNSPEC, &mut t.0) }.is_err() || t.0.is_null() {
        return out;
    }
    // SAFETY: NumEntries rows follow the header in the table the call returned.
    let rows = unsafe { std::slice::from_raw_parts((*t.0).Table.as_ptr(), (*t.0).NumEntries as usize) };
    for r in rows {
        // SAFETY: si_family says which member of the union was written.
        let ip = unsafe {
            match r.Address.si_family {
                AF_INET => fold::v4(r.Address.Ipv4.sin_addr.S_un.S_addr),
                AF_INET6 => IpAddr::V6(r.Address.Ipv6.sin6_addr.u.Byte.into()),
                _ => continue,
            }
        };
        let e = out.entry(r.InterfaceIndex).or_default();
        let s = format!("{ip}/{}", r.OnLinkPrefixLength);
        if ip.is_ipv4() { e.0.push(s) } else { e.1.push(s) }
    }
    for (v4, v6) in out.values_mut() {
        v4.sort();
        v6.sort();
    }
    out
}

/// `None` when the interface table could not be read.
fn interfaces() -> Option<Vec<Row>> {
    let mut t = Mib::<MIB_IF_TABLE2>(std::ptr::null_mut());
    // SAFETY: the call allocates the table; `Mib` frees it.
    if unsafe { GetIfTable2(&mut t.0) }.is_err() || t.0.is_null() {
        return None;
    }
    // SAFETY: NumEntries rows follow the header in the table the call returned.
    let rows = unsafe { std::slice::from_raw_parts((*t.0).Table.as_ptr(), (*t.0).NumEntries as usize) };
    let rows = rows
        .iter()
        .map(|r| {
            let flags = r.InterfaceAndOperStatusFlags._bitfield;
            let n = (r.PhysicalAddressLength as usize).min(r.PhysicalAddress.len());
            Row {
                index: r.InterfaceIndex,
                guid: r.InterfaceGuid.to_u128(),
                alias: wide(&r.Alias),
                desc: wide(&r.Description),
                mac: r.PhysicalAddress[..n].to_vec(),
                mtu: r.Mtu,
                ty: r.Type,
                medium: r.PhysicalMediumType.0 as u32,
                hardware: flags & 1 != 0,
                filter: flags & 2 != 0,
                connector: flags & 4 != 0,
                // IfOperStatusUp.
                oper_up: r.OperStatus.0 == 1,
                speed: r.ReceiveLinkSpeed,
                rx: r.InOctets,
                tx: r.OutOctets,
            }
        })
        .collect();
    Some(rows)
}

pub fn nics() -> Vec<Nic> {
    interfaces().map(|rows| fold::pick(&rows, &addresses())).unwrap_or_default()
}

/// A buffer of `n` bytes at least, aligned for the tables.
fn buffer(n: u32) -> Vec<u64> {
    vec![0u64; (n as usize).div_ceil(8).max(1)]
}

const ERROR_INSUFFICIENT_BUFFER: u32 = 122;

/// Call `get` with a growing buffer until the table fits; `None` on failure.
fn table(get: impl Fn(*mut core::ffi::c_void, &mut u32) -> u32) -> Option<Vec<u64>> {
    let mut size = 0u32;
    for _ in 0..4 {
        // Room for sockets opened between the calls.
        let mut buf = buffer(size + 4096);
        size = (buf.len() * 8) as u32;
        match get(buf.as_mut_ptr().cast(), &mut size) {
            0 => return Some(buf),
            ERROR_INSUFFICIENT_BUFFER => continue,
            _ => return None,
        }
    }
    None
}

fn tcp(af: u16) -> Option<Vec<u64>> {
    // SAFETY: the buffer is `size` bytes; the call writes no more.
    table(|p, size| unsafe { GetExtendedTcpTable(Some(p), size, false, u32::from(af), TCP_TABLE_OWNER_PID_ALL, 0) })
}

fn udp(af: u16) -> Option<Vec<u64>> {
    // SAFETY: as in `tcp`.
    table(|p, size| unsafe { GetExtendedUdpTable(Some(p), size, false, u32::from(af), UDP_TABLE_OWNER_PID, 0) })
}

/// The rows that follow a table's count.
///
/// SAFETY: `first` points at `count` rows the call wrote, inside a buffer
/// that outlives the slice.
unsafe fn rows<'a, R>(count: u32, first: *const R) -> &'a [R] {
    // SAFETY: the caller's promise.
    unsafe { std::slice::from_raw_parts(first, count as usize) }
}

pub fn conns() -> Conns {
    let mut list: Vec<Conn> = Vec::new();
    let mut read_any = false;
    if let Some(buf) = tcp(AF_INET.0) {
        read_any = true;
        let t = buf.as_ptr().cast::<MIB_TCPTABLE_OWNER_PID>();
        // SAFETY: the buffer holds the table GetExtendedTcpTable wrote.
        for r in unsafe { rows((*t).dwNumEntries, (*t).table.as_ptr()) } {
            let remote = Some((fold::v4(r.dwRemoteAddr), r.dwRemotePort));
            list.push(fold::conn(Proto::Tcp, (fold::v4(r.dwLocalAddr), r.dwLocalPort), remote, Some(r.dwState), r.dwOwningPid));
        }
    }
    if let Some(buf) = tcp(AF_INET6.0) {
        read_any = true;
        let t = buf.as_ptr().cast::<MIB_TCP6TABLE_OWNER_PID>();
        // SAFETY: as above.
        for r in unsafe { rows((*t).dwNumEntries, (*t).table.as_ptr()) } {
            let remote = Some((fold::v6(r.ucRemoteAddr), r.dwRemotePort));
            list.push(fold::conn(Proto::Tcp6, (fold::v6(r.ucLocalAddr), r.dwLocalPort), remote, Some(r.dwState), r.dwOwningPid));
        }
    }
    if let Some(buf) = udp(AF_INET.0) {
        read_any = true;
        let t = buf.as_ptr().cast::<MIB_UDPTABLE_OWNER_PID>();
        // SAFETY: as above, from GetExtendedUdpTable.
        for r in unsafe { rows((*t).dwNumEntries, (*t).table.as_ptr()) } {
            list.push(fold::conn(Proto::Udp, (fold::v4(r.dwLocalAddr), r.dwLocalPort), None, None, r.dwOwningPid));
        }
    }
    if let Some(buf) = udp(AF_INET6.0) {
        read_any = true;
        let t = buf.as_ptr().cast::<MIB_UDP6TABLE_OWNER_PID>();
        // SAFETY: as above.
        for r in unsafe { rows((*t).dwNumEntries, (*t).table.as_ptr()) } {
            list.push(fold::conn(Proto::Udp6, (fold::v6(r.ucLocalAddr), r.dwLocalPort), None, None, r.dwOwningPid));
        }
    }
    Conns { list, source: "iphlpapi", note: (!read_any).then_some(Note::Failed) }
}
