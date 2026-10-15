//! Command strings (`screen cpu`, `arrow -1`, `resize left`) to [`Act`].

use ito_common::ScaleTo;
use ito_layout::Toward;

use crate::screen::Screen;
use crate::svcctl::Action;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Goto {
    To(Screen),
    /// The next built screen, wrapping.
    Next,
    Prev,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Step {
    By(i64),
    Top,
    Bot,
    /// A share of the visible rows: `arrow 50%`, `arrow -100%`.
    Page(f32),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Act {
    Screen(Goto),
    Arrow(Step),
    /// The column to the left (-1) or right (+1).
    Focus(i32),
    /// Move the focused column's divider.
    Resize(Toward),
    /// Every column of this screen back to the same share.
    Equalize,
    /// Open what is selected: a device's screen, a process in procs.
    Enter,
    Filter,
    Sort,
    /// procs as a tree of parents and children, or a flat list.
    Tree,
    /// procs' scope: every process, or only mine (`Some(true)`); `None`
    /// switches to the other.
    Scope(Option<bool>),
    /// Ask, then end the selected process; on svc or containers, stop the
    /// service or container.
    Kill,
    /// One step more urgent (`true`) or less urgent priority for the selected process.
    Priority(bool),
    /// Choose the CPUs the selected process may run on.
    Affinity,
    /// Ask, then start, stop or restart the selected service.
    Service(Action),
    /// Every machine of `[[host]]` at once, or back to the screen.
    Fleet,
    /// The next (+1) or previous (-1) machine: this one, then the
    /// `[[host]]` that are up.
    Machine(i32),
    Pause,
    /// One step faster (-1) or slower (+1) sampling.
    Interval(i32),
    Help,
    /// The command palette (`Ctrl+Shift+P`).
    Palette,
    /// The settings screen, open or closed.
    Settings,
    /// Copy what is selected: on fleet, the sample `[[host]]` or the
    /// machine's address.
    Copy,
    /// The settings screen's next (+1) or previous (-1) page.
    Page(i32),
    /// The settings screen's search.
    Search,
    /// The scale every uchmk app shares (common.toml's `scale`).
    Scale(ScaleTo),
    Quit,
    Escape,
    Confirm,
    Backspace,
    /// Nothing: what a key taken from a built-in operation is bound to (the
    /// settings screen's Keys page writes `run = "noop"`), so it no longer
    /// runs the built-in one.
    Noop,
}

pub fn parse(s: &str) -> Result<Act, String> {
    let mut it = s.split_whitespace();
    let name = it.next().ok_or("empty command")?;
    let arg = it.next();
    if let Some(extra) = it.next() {
        return Err(format!("`{s}`: unexpected `{extra}`"));
    }
    let bad = || format!("`{s}`: bad argument");
    let need = || arg.ok_or_else(|| format!("`{s}`: missing argument"));
    let none = |a: Act| if arg.is_some() { Err(format!("`{s}`: takes no argument")) } else { Ok(a) };
    match name {
        "screen" => Ok(Act::Screen(match need()? {
            "next" => Goto::Next,
            "prev" => Goto::Prev,
            n => Goto::To(Screen::from_name(n).ok_or_else(bad)?),
        })),
        "arrow" => {
            let a = need()?;
            Ok(Act::Arrow(match a {
                "top" => Step::Top,
                "bot" | "bottom" => Step::Bot,
                _ if a.ends_with('%') => Step::Page(a[..a.len() - 1].parse::<f32>().map_err(|_| bad())? / 100.0),
                _ => Step::By(a.parse().map_err(|_| bad())?),
            }))
        }
        "focus" => Ok(Act::Focus(match need()? {
            "left" | "-1" => -1,
            "right" | "+1" | "1" => 1,
            _ => return Err(bad()),
        })),
        "resize" => Ok(Act::Resize(match need()? {
            "left" => Toward::Left,
            "right" => Toward::Right,
            "up" => Toward::Up,
            "down" => Toward::Down,
            _ => return Err(bad()),
        })),
        "interval" => Ok(Act::Interval(match need()? {
            "+" | "slower" => 1,
            "-" | "faster" => -1,
            _ => return Err(bad()),
        })),
        "scope" => Ok(Act::Scope(match arg {
            None => None,
            Some("all") => Some(false),
            Some("mine") => Some(true),
            Some(_) => return Err(bad()),
        })),
        "service" => Ok(Act::Service(match need()? {
            "start" => Action::Start,
            "stop" => Action::Stop,
            "restart" => Action::Restart,
            _ => return Err(bad()),
        })),
        "page" => Ok(Act::Page(match need()? {
            "next" | "+1" | "1" => 1,
            "prev" | "-1" => -1,
            _ => return Err(bad()),
        })),
        "machine" => Ok(Act::Machine(match need()? {
            "next" | "+1" | "1" => 1,
            "prev" | "-1" => -1,
            _ => return Err(bad()),
        })),
        "scale" => Ok(Act::Scale(match need()? {
            "in" | "+" => ScaleTo::In,
            "out" | "-" => ScaleTo::Out,
            "reset" | "0" => ScaleTo::Reset,
            _ => return Err(bad()),
        })),
        "equalize" => none(Act::Equalize),
        "enter" => none(Act::Enter),
        "filter" => none(Act::Filter),
        "sort" => none(Act::Sort),
        "tree" => none(Act::Tree),
        "kill" => none(Act::Kill),
        "priority" => Ok(Act::Priority(match need()? {
            "raise" | "up" => true,
            "lower" | "down" => false,
            _ => return Err(bad()),
        })),
        "affinity" => none(Act::Affinity),
        "pause" => none(Act::Pause),
        "fleet" => none(Act::Fleet),
        "help" => none(Act::Help),
        "palette" => none(Act::Palette),
        "settings" => none(Act::Settings),
        "copy" => none(Act::Copy),
        "search" => none(Act::Search),
        "quit" => none(Act::Quit),
        "escape" | "close" => none(Act::Escape),
        "confirm" => none(Act::Confirm),
        "backspace" => none(Act::Backspace),
        "noop" => none(Act::Noop),
        _ => Err(format!("unknown command `{name}`")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_commands() {
        assert_eq!(parse("screen cpu"), Ok(Act::Screen(Goto::To(Screen::Cpu))));
        assert_eq!(parse("screen next"), Ok(Act::Screen(Goto::Next)));
        assert_eq!(parse("arrow -1"), Ok(Act::Arrow(Step::By(-1))));
        assert_eq!(parse("arrow 50%"), Ok(Act::Arrow(Step::Page(0.5))));
        assert_eq!(parse("arrow bot"), Ok(Act::Arrow(Step::Bot)));
        assert_eq!(parse("resize up"), Ok(Act::Resize(Toward::Up)));
        assert_eq!(parse("interval +"), Ok(Act::Interval(1)));
        assert_eq!(parse("focus left"), Ok(Act::Focus(-1)));
        assert_eq!(parse("kill"), Ok(Act::Kill));
        assert_eq!(parse("priority raise"), Ok(Act::Priority(true)));
        assert_eq!(parse("priority lower"), Ok(Act::Priority(false)));
        assert!(parse("priority").is_err());
        assert_eq!(parse("affinity"), Ok(Act::Affinity));
        assert!(parse("affinity 1").is_err());
        assert_eq!(parse("tree"), Ok(Act::Tree));
        assert_eq!(parse("fleet"), Ok(Act::Fleet));
        assert!(parse("fleet nas").is_err());
        assert_eq!(parse("machine next"), Ok(Act::Machine(1)));
        assert_eq!(parse("machine prev"), Ok(Act::Machine(-1)));
        assert!(parse("machine nas").is_err());
        assert_eq!(parse("settings"), Ok(Act::Settings));
        assert_eq!(parse("copy"), Ok(Act::Copy));
        assert_eq!(parse("noop"), Ok(Act::Noop));
        assert!(parse("noop 1").is_err());
        assert_eq!(parse("page next"), Ok(Act::Page(1)));
        assert_eq!(parse("page -1"), Ok(Act::Page(-1)));
        assert!(parse("page").is_err());
        assert_eq!(parse("search"), Ok(Act::Search));
        assert_eq!(parse("scale in"), Ok(Act::Scale(ScaleTo::In)));
        assert_eq!(parse("scale -"), Ok(Act::Scale(ScaleTo::Out)));
        assert_eq!(parse("scale reset"), Ok(Act::Scale(ScaleTo::Reset)));
        assert!(parse("scale").is_err());
        assert!(parse("scale up").is_err());
        assert_eq!(parse("scope"), Ok(Act::Scope(None)));
        assert_eq!(parse("scope mine"), Ok(Act::Scope(Some(true))));
        assert_eq!(parse("scope all"), Ok(Act::Scope(Some(false))));
        assert!(parse("scope others").is_err());
        assert_eq!(parse("service restart"), Ok(Act::Service(Action::Restart)));
        assert_eq!(parse("service stop"), Ok(Act::Service(Action::Stop)));
        assert!(parse("service").is_err());
        assert!(parse("service pause").is_err());
        assert!(parse("screen nowhere").is_err());
        assert!(parse("quit now").is_err());
        assert!(parse("fly").is_err());
    }
}
