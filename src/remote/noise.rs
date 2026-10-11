//! Direct connections: a viewer reaches an agent across the network without
//! ssh or Tailscale, over the Noise protocol (`Noise_IK_25519_ChaChaPoly_BLAKE2s`,
//! pure Rust through `snow`). Keys are swapped beforehand, the way WireGuard
//! does it: each machine has a key pair in its config folder (`key`, the
//! private half in base64), the viewer lists the agent's public key in its
//! `[[host]] key` and the agent lists the viewer's in `[[agent.viewer]]`.
//! No certificates and no passwords.
//!
//! The viewer starts (IK: it knows the agent's key, and its own goes over
//! encrypted in the first message), the agent looks the viewer's key up
//! before it answers. Every message is a big-endian `u16` length and the
//! bytes; after the handshake each carries up to [`CHUNK`] bytes of the same
//! frames a plain connection carries ([`super::proto`]). An agent that will
//! not go on answers the first message with one byte that says why
//! ([`Refused`]), so the viewer can tell its card.

use std::fmt;
use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use snow::StatelessTransportState;
use snow::resolvers::{CryptoResolver, DefaultResolver};

/// The handshake and ciphers.
const PARAMS: &str = "Noise_IK_25519_ChaChaPoly_BLAKE2s";
/// The longest Noise message, and the tag each one carries.
const MAX_MSG: usize = 65535;
const TAG: usize = 16;
/// Plain bytes in one message.
pub const CHUNK: usize = MAX_MSG - TAG;
/// The viewer's first message with nothing in it: its ephemeral key, its
/// static key and the tags. Anything else is not a direct viewer.
const FIRST: usize = 32 + (32 + TAG) + TAG;
/// A peer that says nothing for this long during the handshake is not one.
const HANDSHAKE: Duration = Duration::from_secs(10);
/// The private key's file in yagura's config folder.
pub const KEY_FILE: &str = "key";

/// A Curve25519 key, private or public.
pub type Key = [u8; 32];

/// A key as config.toml writes it: 44 characters of base64.
pub fn encode(k: &Key) -> String {
    B64.encode(k)
}

pub fn decode(s: &str) -> Option<Key> {
    B64.decode(s.trim()).ok()?.try_into().ok()
}

/// This machine's key pair.
#[derive(Clone, PartialEq, Eq)]
pub struct Keys {
    private: Key,
    pub public: Key,
}

impl fmt::Debug for Keys {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Keys({})", encode(&self.public))
    }
}

impl Keys {
    /// A new random pair.
    pub fn generate() -> io::Result<Self> {
        let kp = snow::Builder::new(params()).generate_keypair().map_err(io::Error::other)?;
        Self::from_private(kp.private.as_slice().try_into().map_err(io::Error::other)?)
    }

    /// The pair whose private half is `private`.
    pub fn from_private(private: Key) -> io::Result<Self> {
        let mut dh = DefaultResolver.resolve_dh(&params().dh).ok_or_else(|| io::Error::other("no Curve25519"))?;
        dh.set(&private);
        let public = dh.pubkey().try_into().map_err(io::Error::other)?;
        Ok(Self { private, public })
    }
}

fn params() -> snow::params::NoiseParams {
    PARAMS.parse().expect("a known Noise pattern")
}

/// Where this machine's private key is kept.
pub fn key_path(dir: &Path) -> PathBuf {
    dir.join(KEY_FILE)
}

/// This machine's keys from yagura's config folder `dir`, made and written
/// there the first time (readable by this account alone on Unix). A file
/// that does not hold a key is an error rather than written over: another
/// machine may know its public half.
pub fn load_or_make(dir: &Path) -> io::Result<Keys> {
    // The window's host threads ask at once; one makes it, the rest read it.
    static ONE: Mutex<()> = Mutex::new(());
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let path = key_path(dir);
    let read = |path: &Path| -> io::Result<Keys> {
        let text = std::fs::read_to_string(path)?;
        let key = decode(&text).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, format!("{}: not a yagura key (44 characters of base64)", path.display()))
        })?;
        Keys::from_private(key)
    };
    match read(&path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        other => return other,
    }
    std::fs::create_dir_all(dir)?;
    let keys = Keys::generate()?;
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
    match opts.open(&path) {
        Ok(mut f) => {
            writeln!(f, "{}", encode(&keys.private))?;
            f.sync_all()?;
            Ok(keys)
        }
        // Another yagura (the agent, or the window) made it just now.
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            for _ in 0..20 {
                if let Ok(k) = read(&path) {
                    return Ok(k);
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            read(&path)
        }
        Err(e) => Err(e),
    }
}

/// This machine's name, for the snippets.
pub fn host_name() -> String {
    sysinfo::System::host_name().filter(|h| !h.is_empty()).unwrap_or_else(|| "this-machine".into())
}

/// The `[[agent.viewer]]` that lets the machine `host` with the public key
/// `key` in: it goes into the config.toml of the machine watched.
pub fn viewer_snippet(host: &str, key: &str) -> String {
    format!("[[agent.viewer]]\nname = \"{host}\"\nkey = \"{key}\"\n")
}

/// The `[agent] listen` that takes direct connections on every address.
pub fn listen_snippet() -> String {
    format!("[agent]\nlisten = \"0.0.0.0:{}\"\n", super::proto::DIRECT_PORT)
}

/// The `[[host]]` that watches the machine `host` with the public key `key`
/// directly: it goes into the watching machine's config.toml.
pub fn host_snippet(host: &str, key: &str) -> String {
    format!("[[host]]\nname = \"{host}\"\naddr = \"{host}\"\nkey = \"{key}\"\n")
}

/// What `yagura --key` prints: this machine's public key and the snippets
/// that use it, one for each way round.
pub fn howto(public: &Key, path: &Path, host: &str) -> String {
    let key = encode(public);
    let indent = |s: String| s.lines().map(|l| format!("  {l}\n")).collect::<String>();
    format!(
        "This machine's public key (the private half stays in {path}):

  {key}

To watch other machines from this one, add this to the config.toml of each
of them; their agent then lets this machine in:

{viewer}
To watch this machine from another, put this in this machine's config.toml
and run `yagura --agent` (or --install-service) here:

{listen}
and this in the other machine's config.toml (the address as it reaches here):

{host}",
        path = path.display(),
        viewer = indent(viewer_snippet(host, &key)),
        listen = indent(listen_snippet()),
        host = indent(host_snippet(host, &key)),
    )
}

/// Why an agent ended a handshake: the byte it answers with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    /// Its `[[agent.viewer]]` does not list the viewer's key.
    UnknownViewer = 1,
    /// The viewer's first message does not open with the agent's key: the
    /// viewer's `[[host]] key` is another machine's (or an old one).
    WrongAgentKey = 2,
}

impl Refused {
    fn of(b: u8) -> Option<Self> {
        match b {
            1 => Some(Refused::UnknownViewer),
            2 => Some(Refused::WrongAgentKey),
            _ => None,
        }
    }
}

/// How a viewer's handshake ended.
#[derive(Debug)]
pub enum Fail {
    Refused(Refused),
    Io(io::Error),
}

impl From<io::Error> for Fail {
    fn from(e: io::Error) -> Self {
        Fail::Io(e)
    }
}

fn bad(e: impl fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, format!("noise: {e}"))
}

fn read_msg(s: &mut impl Read) -> io::Result<Vec<u8>> {
    let mut len = [0; 2];
    s.read_exact(&mut len)?;
    let mut msg = vec![0; usize::from(u16::from_be_bytes(len))];
    s.read_exact(&mut msg)?;
    Ok(msg)
}

fn write_msg(s: &mut impl Write, msg: &[u8]) -> io::Result<()> {
    let mut out = Vec::with_capacity(2 + msg.len());
    out.extend_from_slice(&(msg.len() as u16).to_be_bytes());
    out.extend_from_slice(msg);
    s.write_all(&out)?;
    s.flush()
}

/// The receiving half of a connection: plain bytes, or the messages opened.
pub struct Reader {
    s: TcpStream,
    t: Option<Arc<StatelessTransportState>>,
    nonce: u64,
    buf: Vec<u8>,
    pos: usize,
}

/// The sending half: each write seals up to [`CHUNK`] bytes in a message.
pub struct Writer {
    s: TcpStream,
    t: Option<Arc<StatelessTransportState>>,
    nonce: u64,
    out: Vec<u8>,
}

impl Read for Reader {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let Some(t) = &self.t else { return self.s.read(out) };
        while self.pos == self.buf.len() {
            let msg = read_msg(&mut self.s)?;
            self.buf.resize(msg.len(), 0);
            let n = t.read_message(self.nonce, &msg, &mut self.buf).map_err(bad)?;
            self.nonce += 1;
            self.buf.truncate(n);
            self.pos = 0;
        }
        let n = out.len().min(self.buf.len() - self.pos);
        out[..n].copy_from_slice(&self.buf[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

impl Write for Writer {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        let Some(t) = &self.t else { return self.s.write(data) };
        let n = data.len().min(CHUNK);
        self.out.resize(2 + n + TAG, 0);
        let len = t.write_message(self.nonce, &data[..n], &mut self.out[2..]).map_err(bad)?;
        self.nonce += 1;
        self.out[..2].copy_from_slice(&(len as u16).to_be_bytes());
        self.s.write_all(&self.out[..2 + len])?;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.s.flush()
    }
}

/// A connection without Noise (127.0.0.1, or through a tunnel).
pub fn plain(s: &TcpStream) -> io::Result<(Reader, Writer)> {
    halves(s, None)
}

fn halves(s: &TcpStream, t: Option<StatelessTransportState>) -> io::Result<(Reader, Writer)> {
    let t = t.map(Arc::new);
    let r = Reader { s: s.try_clone()?, t: t.clone(), nonce: 0, buf: Vec::new(), pos: 0 };
    let w = Writer { s: s.try_clone()?, t, nonce: 0, out: Vec::new() };
    Ok((r, w))
}

/// The viewer's side: a handshake with the agent whose public key is
/// `agent`. The read timeout of `s` is left at [`HANDSHAKE`].
pub fn initiate(s: &TcpStream, keys: &Keys, agent: &Key) -> Result<(Reader, Writer), Fail> {
    let mut hs = snow::Builder::new(params())
        .local_private_key(&keys.private)
        .and_then(|b| b.remote_public_key(agent))
        .and_then(|b| b.build_initiator())
        .map_err(bad)?;
    s.set_read_timeout(Some(HANDSHAKE))?;
    let mut io = s;
    let mut buf = vec![0; MAX_MSG];
    let n = hs.write_message(&[], &mut buf).map_err(bad)?;
    write_msg(&mut io, &buf[..n])?;
    let reply = read_msg(&mut io)?;
    if let [b] = reply[..] {
        return Err(Refused::of(b).map_or_else(|| Fail::Io(bad(format!("refused ({b})"))), Fail::Refused));
    }
    hs.read_message(&reply, &mut buf).map_err(bad)?;
    let t = hs.into_stateless_transport_mode().map_err(bad)?;
    Ok(halves(s, Some(t))?)
}

/// The agent's side: a handshake with a viewer whose key `known` finds (its
/// name); a viewer it does not know is told so and its key given in the
/// error, for the agent's log. The read timeout of `s` is left at
/// [`HANDSHAKE`].
pub fn respond<N>(s: &TcpStream, keys: &Keys, known: impl Fn(&Key) -> Option<N>) -> io::Result<(N, Reader, Writer)> {
    let mut hs = snow::Builder::new(params()).local_private_key(&keys.private).and_then(|b| b.build_responder()).map_err(bad)?;
    s.set_read_timeout(Some(HANDSHAKE))?;
    let mut io = s;
    // Read the length first: a plain viewer's frame is not waited out.
    let mut len = [0; 2];
    io.read_exact(&mut len)?;
    if usize::from(u16::from_be_bytes(len)) != FIRST {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "not a direct viewer (a plain one connects to the 127.0.0.1 port)"));
    }
    let mut first = vec![0; FIRST];
    io.read_exact(&mut first)?;
    let mut buf = vec![0; MAX_MSG];
    if hs.read_message(&first, &mut buf).is_err() {
        let _ = write_msg(&mut io, &[Refused::WrongAgentKey as u8]);
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "a viewer with another key for this agent"));
    }
    let theirs: Key = hs.get_remote_static().and_then(|k| k.try_into().ok()).ok_or_else(|| bad("no viewer key"))?;
    let Some(name) = known(&theirs) else {
        let _ = write_msg(&mut io, &[Refused::UnknownViewer as u8]);
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, format!("an unknown viewer key {}", encode(&theirs))));
    };
    let n = hs.write_message(&[], &mut buf).map_err(bad)?;
    write_msg(&mut io, &buf[..n])?;
    let t = hs.into_stateless_transport_mode().map_err(bad)?;
    let (r, w) = halves(s, Some(t))?;
    Ok((name, r, w))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::test_dir;
    use std::net::TcpListener;

    fn pair() -> (TcpStream, TcpStream) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let c = TcpStream::connect(l.local_addr().unwrap()).unwrap();
        (c, l.accept().unwrap().0)
    }

    #[test]
    fn the_howto_gives_snippets_that_read_back() {
        let k = Keys::generate().unwrap();
        let text = howto(&k.public, Path::new("/x/key"), "nas");
        assert!(text.contains(&encode(&k.public)) && !text.contains(&encode(&k.private)), "{text}");
        let d = test_dir("noise-howto");
        let cfg = d.join("yagura");
        std::fs::create_dir_all(&cfg).unwrap();
        // Each snippet as config.toml.
        let parts: Vec<&str> = text.split("\n\n").filter(|p| p.starts_with("  [")).collect();
        assert_eq!(parts.len(), 3, "{text}");
        let toml = parts.iter().map(|p| p.lines().map(str::trim).collect::<Vec<_>>().join("\n")).collect::<Vec<_>>().join("\n");
        std::fs::write(cfg.join("config.toml"), toml).unwrap();
        let s = crate::config::settings::load(Some(&d));
        assert!(s.warnings.is_empty(), "{:?}", s.warnings);
        assert_eq!(s.viewers.len(), 1);
        assert_eq!((s.viewers[0].name.as_str(), s.viewers[0].key), ("nas", k.public));
        assert_eq!(s.listen.as_deref(), Some("0.0.0.0:7314"));
        assert_eq!((s.hosts.len(), s.hosts[0].key), (1, Some(k.public)));
    }

    #[test]
    fn keys_read_back_and_derive_their_public_half() {
        let k = Keys::generate().unwrap();
        assert_eq!(Keys::from_private(k.private).unwrap(), k);
        assert_ne!(k.private, k.public);
        let text = encode(&k.public);
        assert_eq!(text.len(), 44);
        assert_eq!(decode(&format!(" {text}\n")), Some(k.public));
        assert_eq!(decode("short"), None);
        assert_eq!(decode(&B64.encode([0u8; 31])), None);
        assert!(!format!("{k:?}").contains(&encode(&k.private)), "the private half stays out of logs");
    }

    #[test]
    fn the_key_file_is_made_once_and_kept() {
        let d = test_dir("noise-key");
        let a = load_or_make(&d.join("yagura")).unwrap();
        let b = load_or_make(&d.join("yagura")).unwrap();
        assert_eq!(a, b, "the same keys the second time");
        let path = key_path(&d.join("yagura"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }
        std::fs::write(&path, "not a key\n").unwrap();
        assert_eq!(load_or_make(&d.join("yagura")).unwrap_err().kind(), io::ErrorKind::InvalidData);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "not a key\n", "never written over");
    }

    /// The agent's side on a thread: its outcome (the viewer's name).
    fn agent(
        s: TcpStream,
        keys: Keys,
        viewers: Vec<(Key, &'static str)>,
    ) -> std::thread::JoinHandle<io::Result<(&'static str, Reader, Writer)>> {
        std::thread::spawn(move || respond(&s, &keys, |k| viewers.iter().find(|(v, _)| v == k).map(|(_, n)| *n)))
    }

    #[test]
    fn a_known_viewer_talks_both_ways_in_large_frames() {
        let (va, ag) = (Keys::generate().unwrap(), Keys::generate().unwrap());
        let (c, s) = pair();
        let side = agent(s, ag.clone(), vec![(va.public, "laptop")]);
        let (mut r, mut w) = initiate(&c, &va, &ag.public).unwrap();
        let (name, mut ar, mut aw) = side.join().unwrap().unwrap();
        assert_eq!(name, "laptop");
        // Larger than one message, both ways, as frames.
        let big: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
        super::super::proto::write_frame(&mut w, &big).unwrap();
        assert_eq!(super::super::proto::read_frame::<Vec<u8>>(&mut ar).unwrap(), big);
        super::super::proto::write_frame(&mut aw, &"pong").unwrap();
        assert_eq!(super::super::proto::read_frame::<String>(&mut r).unwrap(), "pong");
        // Nothing goes over in the clear.
        let (c2, s2) = pair();
        let side = agent(s2, ag.clone(), vec![(va.public, "laptop")]);
        let (_r2, mut w2) = initiate(&c2, &va, &ag.public).unwrap();
        let (_, mut raw, _) = side.join().unwrap().unwrap();
        raw.t = None;
        w2.write_all(b"secret readings").unwrap();
        let mut got = vec![0; 2 + 15 + TAG];
        raw.read_exact(&mut got).unwrap();
        assert!(!got.windows(7).any(|x| x == b"secret "), "{got:?}");
    }

    #[test]
    fn an_unknown_viewer_is_told_and_its_key_logged() {
        let (va, ag) = (Keys::generate().unwrap(), Keys::generate().unwrap());
        let (c, s) = pair();
        let side = agent(s, ag.clone(), Vec::new());
        assert!(matches!(initiate(&c, &va, &ag.public), Err(Fail::Refused(Refused::UnknownViewer))));
        let e = side.join().unwrap().err().unwrap();
        assert_eq!(e.kind(), io::ErrorKind::PermissionDenied);
        assert!(e.to_string().contains(&encode(&va.public)), "{e}");
    }

    #[test]
    fn a_viewer_with_another_agent_key_is_told() {
        let (va, ag, other) = (Keys::generate().unwrap(), Keys::generate().unwrap(), Keys::generate().unwrap());
        let (c, s) = pair();
        let side = agent(s, ag, vec![(va.public, "laptop")]);
        assert!(matches!(initiate(&c, &va, &other.public), Err(Fail::Refused(Refused::WrongAgentKey))));
        assert_eq!(side.join().unwrap().err().unwrap().kind(), io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn a_plain_viewer_is_turned_away_at_once() {
        let ag = Keys::generate().unwrap();
        let (mut c, s) = pair();
        let side = agent(s, ag, Vec::new());
        super::super::proto::write_frame(&mut c, &super::super::proto::ToAgent::Hello { proto: super::super::proto::PROTO }).unwrap();
        let t = std::time::Instant::now();
        assert_eq!(side.join().unwrap().err().unwrap().kind(), io::ErrorKind::InvalidData);
        assert!(t.elapsed() < Duration::from_secs(5), "not waited out");
    }
}
