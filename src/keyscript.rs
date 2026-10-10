//! `mimamori --keys "2<A-S-Right><Shot:cpu>"`: keys pressed by mimamori
//! itself once it has started, the way filer's `--keys` works, so a test
//! session can drive it without reaching into the window from outside.
//!
//! The keys go in as the `egui::Event`s a keyboard would have produced,
//! through `raw_input_hook`, so they take the same road as a real press.
//! Which events a key becomes is read back out of [`keys::from_egui`]
//! rather than written down a second time.
//!
//! When the script ends, `MIMAMORI_KEYS_DONE` (a path) gets the state a
//! check reads; `<Shot:name>` and `<State:name>` write beside it.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::config::keys::{self, Code, Key};

/// The pause after each step, so the frame that follows a key has drawn
/// before the next key goes in.
const GAP: Duration = Duration::from_millis(60);
/// Between the frames of a click or a drag.
const FRAME: Duration = Duration::from_millis(20);

/// A place in the window, in thousandths of its width and height.
pub type At = (u16, u16);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    Key(Key),
    /// `<Wait:500>`: this many milliseconds before the next step. The
    /// sampler runs at its own pace, so a check that wants numbers on screen
    /// waits for them.
    Wait(Duration),
    /// `<Shot:name>`: the window as it is now, as `name.png`.
    Shot(String),
    /// `<State:name>`: what `MIMAMORI_KEYS_DONE` would say now, as `name.txt`.
    State(String),
    /// `<Quit>`: close the window.
    Quit,
    /// `<Click:0.5,0.4>`: the left button at that place.
    Click(At),
    /// `<Drag:0.2,0.3-0.6,0.3>`: the left button down at the first place,
    /// moved to the second and let go there.
    Drag(At, At),
}

/// What the frame loop does with a step that is not a plain input.
#[derive(Debug, PartialEq, Eq)]
pub enum Do {
    State(String),
    Quit,
    /// The last step has gone in.
    Done,
}

/// `0.5,0.4` as a place in the window.
fn at(text: &str) -> Option<At> {
    let (x, y) = text.split_once(',')?;
    let part = |v: &str| v.trim().parse::<f32>().ok().filter(|v| (0.0..=1.0).contains(v)).map(|v| (v * 1000.0).round() as u16);
    Some((part(x)?, part(y)?))
}

/// The text after `<Name:` in `token`, which must not be empty.
fn arg<'a>(token: &'a str, name: &str) -> Option<&'a str> {
    token.strip_prefix('<')?.strip_suffix('>')?.strip_prefix(name)?.strip_prefix(':').filter(|a| !a.is_empty())
}

fn step(token: &str) -> Result<Step, String> {
    let bad = |what: &str| format!("`{token}`: {what}");
    if token == "<Quit>" {
        return Ok(Step::Quit);
    }
    if let Some(ms) = arg(token, "Wait") {
        return ms
            .parse::<u64>()
            .map(|ms| Step::Wait(Duration::from_millis(ms)))
            .map_err(|_| bad("the wait is in whole milliseconds, as `<Wait:500>`"));
    }
    if let Some(name) = arg(token, "Shot") {
        return Ok(Step::Shot(name.to_string()));
    }
    if let Some(name) = arg(token, "State") {
        return Ok(Step::State(name.to_string()));
    }
    if let Some(place) = arg(token, "Click") {
        return at(place).map(Step::Click).ok_or_else(|| bad("a place is two fractions of the window, as `<Click:0.5,0.4>`"));
    }
    if let Some(places) = arg(token, "Drag") {
        let parsed = places.split_once('-').and_then(|(a, b)| Some(Step::Drag(at(a)?, at(b)?)));
        return parsed.ok_or_else(|| bad("a drag is two places, as `<Drag:0.2,0.3-0.6,0.3>`"));
    }
    Key::parse(token).map(Step::Key).ok_or_else(|| bad("not a key"))
}

/// The steps of a script. A plain space is refused (write `<Space>`), and
/// `<…>*N` repeats a key N times.
pub fn parse(script: &str) -> Result<Vec<Step>, String> {
    let mut out = Vec::new();
    let mut rest = script;
    while let Some(c) = rest.chars().next() {
        if c.is_whitespace() {
            return Err(format!("a plain space cannot be typed here; write `<Space>` (at `{rest}`)"));
        }
        let token = match c {
            '<' => &rest[..=rest.find('>').ok_or_else(|| format!("`{rest}` has no closing `>`"))?],
            _ => &rest[..c.len_utf8()],
        };
        let s = step(token)?;
        rest = &rest[token.len()..];
        let mut times = 1;
        if let (Step::Key(_), Some(after)) = (&s, rest.strip_prefix('*').filter(|_| c == '<')) {
            let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
            times = digits
                .parse::<usize>()
                .ok()
                .filter(|n| (1..=1000).contains(n))
                .ok_or_else(|| format!("`{token}*` takes a count from 1 to 1000"))?;
            rest = &after[digits.len()..];
        }
        out.extend(std::iter::repeat_n(s, times));
    }
    Ok(out)
}

/// The events a key becomes: a bare character as text, anything else as
/// the key event that reads back as it. `None` for a key no keyboard has.
pub fn events(key: &Key) -> Option<Vec<egui::Event>> {
    if let Code::Char(c) = key.code
        && key.is_bare_char()
    {
        return Some(vec![egui::Event::Text(c.to_string())]);
    }
    for &k in egui::Key::ALL {
        for bits in 0..8u8 {
            let modifiers = egui::Modifiers {
                shift: bits & 1 != 0,
                ctrl: bits & 2 != 0,
                command: bits & 2 != 0,
                alt: bits & 4 != 0,
                ..Default::default()
            };
            if keys::from_egui(k, &modifiers).as_ref() == Some(key) {
                let press = |pressed| egui::Event::Key { key: k, physical_key: None, pressed, repeat: false, modifiers };
                return Some(vec![press(true), press(false)]);
            }
        }
    }
    None
}

/// A click or a drag as the frames of pointer events it takes: egui sees a
/// press and a release in one frame as a click, never as a drag.
fn pointer_frames(step: &Step, rect: egui::Rect) -> VecDeque<Vec<egui::Event>> {
    let place = |(x, y): At| rect.min + egui::vec2(rect.width() * x as f32 / 1000.0, rect.height() * y as f32 / 1000.0);
    let button =
        |pos, pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
    match *step {
        Step::Click(a) => {
            let p = place(a);
            [vec![egui::Event::PointerMoved(p)], vec![button(p, true)], vec![button(p, false)]].into()
        }
        Step::Drag(a, b) => {
            let (a, b) = (place(a), place(b));
            let mut frames = VecDeque::from([vec![egui::Event::PointerMoved(a)], vec![button(a, true)]]);
            frames.extend((1..=4).map(|k| vec![egui::Event::PointerMoved(a.lerp(b, k as f32 / 4.0))]));
            frames.push_back(vec![button(b, false)]);
            frames
        }
        _ => VecDeque::new(),
    }
}

/// A script being played.
pub struct Runner {
    steps: VecDeque<Step>,
    next: Instant,
    /// The picture asked for, and whether the request has gone out.
    shot: Option<(String, bool)>,
    /// The frames of a click or a drag still to go in.
    pointer: VecDeque<Vec<egui::Event>>,
    /// Where `<Shot:>` and `<State:>` write.
    pub dir: PathBuf,
    /// `MIMAMORI_KEYS_DONE`.
    pub done: Option<PathBuf>,
    finished: bool,
}

impl Runner {
    pub fn new(steps: Vec<Step>, done: Option<PathBuf>) -> Self {
        let dir = done
            .as_ref()
            .and_then(|d| d.parent().map(PathBuf::from))
            .filter(|d| !d.as_os_str().is_empty())
            .unwrap_or_else(|| PathBuf::from("."));
        // The first frames lay the window out; a key before them can land
        // before the panes exist.
        Self {
            steps: steps.into(),
            next: Instant::now() + Duration::from_millis(500),
            shot: None,
            pointer: VecDeque::new(),
            dir,
            done,
            finished: false,
        }
    }

    /// Put the next step's events into this frame's input when it is due.
    pub fn input(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) -> Option<Do> {
        if self.finished || self.shot.is_some() {
            return None;
        }
        let now = Instant::now();
        if now < self.next {
            return None;
        }
        if let Some(frame) = self.pointer.pop_front() {
            raw.events.extend(frame);
            self.next = now + FRAME;
            return None;
        }
        let Some(step) = self.steps.pop_front() else {
            self.finished = true;
            return Some(Do::Done);
        };
        self.next = now + GAP;
        match step {
            Step::Key(k) => raw.events.extend(events(&k).unwrap_or_default()),
            Step::Wait(d) => self.next = now + d,
            Step::Shot(name) => self.shot = Some((name, false)),
            Step::State(name) => return Some(Do::State(name)),
            Step::Quit => return Some(Do::Quit),
            Step::Click(_) | Step::Drag(..) => {
                let rect = raw.screen_rect.unwrap_or_else(|| ctx.content_rect());
                self.pointer = pointer_frames(&step, rect);
                self.next = now;
            }
        }
        None
    }

    /// When the next frame is wanted for the script: a repaint asked for in
    /// `raw_input_hook` is forgotten when the frame begins, so the frame asks.
    pub fn due(&self) -> Option<Duration> {
        match () {
            _ if self.finished => None,
            _ if self.shot.is_some() => Some(Duration::ZERO),
            _ => Some(self.next.saturating_duration_since(Instant::now())),
        }
    }

    /// `<Shot:name>`, once per frame: ask for the picture, then save it when
    /// it arrives. An error is returned for a toast.
    pub fn shot(&mut self, ctx: &egui::Context) -> Option<String> {
        let (name, sent) = self.shot.clone()?;
        if !sent {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new(name.clone())));
            self.shot = Some((name, true));
            return None;
        }
        let image = ctx.input(|i| {
            i.raw.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { user_data, image, .. }
                    if user_data.data.as_ref().and_then(|d| d.downcast_ref::<String>()) == Some(&name) =>
                {
                    Some(image.clone())
                }
                _ => None,
            })
        });
        let image = image?;
        self.shot = None;
        self.next = Instant::now() + GAP;
        let path = self.dir.join(format!("{name}.png"));
        save_png(&path, &image).err().map(|e| format!("Shot {name}: {e}"))
    }
}

fn save_png(path: &std::path::Path, image: &egui::ColorImage) -> Result<(), String> {
    let [w, h] = image.size;
    let rgba: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_srgba_unmultiplied()).collect();
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w as u32, h as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(&rgba).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripts_parse_into_steps() {
        let s = parse("2<A-S-Right>*2<Wait:300><Shot:cpu><State:a><Click:0.5,0.25><Drag:0.2,0.3-0.6,0.3><Quit>").unwrap();
        assert_eq!(s[0], Step::Key(Key::char('2')));
        assert_eq!(s[1], s[2]);
        assert!(matches!(s[1], Step::Key(k) if k.alt && k.shift));
        assert_eq!(s[3], Step::Wait(Duration::from_millis(300)));
        assert_eq!(s[4], Step::Shot("cpu".into()));
        assert_eq!(s[5], Step::State("a".into()));
        assert_eq!(s[6], Step::Click((500, 250)));
        assert_eq!(s[7], Step::Drag((200, 300), (600, 300)));
        assert_eq!(s[8], Step::Quit);
        assert_eq!(s.len(), 9);
    }

    #[test]
    fn bad_scripts_are_refused() {
        assert!(parse("j k").is_err(), "a plain space");
        assert!(parse("<Wait:soon>").is_err());
        assert!(parse("<Click:2,0>").is_err());
        assert!(parse("<Tab").is_err());
        assert!(parse("<j>*0").is_err());
    }

    #[test]
    fn keys_become_the_events_that_read_back_as_them() {
        assert_eq!(events(&Key::char('j')), Some(vec![egui::Event::Text("j".into())]));
        let k = Key::parse("<A-S-Right>").unwrap();
        let ev = events(&k).unwrap();
        assert!(
            matches!(ev[0], egui::Event::Key { key: egui::Key::ArrowRight, pressed: true, modifiers, .. } if modifiers.alt && modifiers.shift)
        );
    }
}
