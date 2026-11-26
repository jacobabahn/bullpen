//! The list's rows as values: what each row of the board's list says and how it should
//! look, worked out once from the board's state. The text list (list.rs, later.rs) and
//! the image (image/) both draw from these, so they never read the tasks two ways.

use super::later::later_rows;
use crate::model::{self, width, Item, Layout, Task, STALE};
use crate::App;

pub(crate) type Now = chrono::DateTime<chrono::FixedOffset>;

/// One row of the list.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum Row {
    /// a group's heading and how many it has
    Head { name: String, count: usize },
    Task(TaskRow),
    /// a line under a task
    Detail(DetailRow),
    Gap,
    /// what a board with no tasks to show says instead; the first line bold
    Empty { text: String, first: bool },
    /// the Later group folded to one line; amber when something there waits on you
    Later { text: String, amber: bool },
    /// an item of the Later group, expanded
    Item(ItemRow),
    /// a line under an item, the most lines it takes (detail_lines), and whether the
    /// item is selected (the text list's: the selected item's; the image's: any item's
    /// with something to know)
    ItemDetail(String, usize, bool),
    /// a repository's heading in mission control's image look: its name, where it is,
    /// its coordinators (never on a board)
    Repo(RepoRow),
}

/// A repository's heading (mission control's): its name, its folder and how many tasks
/// are open, and each coordinator's ID and state; a blocked one's question under it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct RepoRow {
    pub name: String,
    /// "~/dev/bullpen · 5 open"
    pub place: String,
    /// each coordinator's words ("c4 working", or "no coordinator") and its state's word
    /// (working, blocked, idle, gone, unknown: core's `Coordinator::word`)
    pub coordinators: Vec<(String, &'static str)>,
    /// what a blocked coordinator asks, under the heading: it waits on you
    pub question: Option<String>,
}

/// A task's mark: still, or one the text board turns.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum Mark {
    /// a command running on it (accept, drop, start, resume): the spinner
    Job,
    /// its tour being written: the spinner, in its status's colour
    Tour(String),
    /// an agent gone quiet: its status's mark, in the colour of one that needs you
    Stuck(String),
    /// working: turns on the text board
    Working,
    /// in review: turns on the text board
    Review,
    /// its status's mark, in its colour (one at the usage limit too)
    Still(String),
}

/// The bar beside a task that needs you (amber), or whose agent stopped (red).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Flag {
    Needs,
    Stopped,
}

/// A task's row: its mark, ID, title, risk and age.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct TaskRow {
    pub id: String,
    /// as the board shows it (another coordinator's starts with whose), not cut to fit
    pub title: String,
    pub mark: Mark,
    /// it came from the backlog (↻)
    pub from_backlog: bool,
    pub flag: Option<Flag>,
    /// its age; "y / n" while it's asked about; empty when its detail line has it
    pub age: String,
    /// a ready task's risk level
    pub risk: Option<String>,
    /// it has needed you STALE seconds: its age is red
    pub stale: bool,
    pub selected: bool,
    /// asked y/n about: Some(true) when what's asked is risky (red), else green
    pub asked: Option<bool>,
    /// its title is dim: closed, its coordinator's, or another coordinator's
    pub quiet: bool,
    /// it waits on you (it needs you, stopped, or is ready) and isn't another
    /// coordinator's: the image board draws its title in the foreground
    pub yours: bool,
    /// mission control's selected task (not asked about): what ⏎ does, which the
    /// image look puts in a cap at its card's lower right (the board's key row says
    /// its tasks' keys)
    pub key: Option<(String, String)>,
    /// the repository it's in, before its title, dim: mission control's inbox, which
    /// lists every repository's; empty on a board
    pub repo: String,
    /// on mission control, the state folder of the repository it's in (IDs repeat across
    /// repositories); empty on a board
    pub dir: String,
}

impl TaskRow {
    /// What tells this task from every other on the screen: its ID, and on mission
    /// control its repository's.
    pub(crate) fn who(&self) -> String {
        if self.dir.is_empty() { self.id.clone() } else { format!("{}\n{}", self.dir, self.id) }
    }
}

/// A line under a task.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct DetailRow {
    pub text: String,
    /// the most lines it takes: the text list's are wrapped already (1), the image's
    /// whole, for it to wrap by pixels (detail_lines)
    pub lines: usize,
    /// at normal weight: it says what the task needs from you
    pub loud: bool,
    /// its first columns are the task's age (a narrow board's)
    pub lead: usize,
    /// its task is selected
    pub selected: bool,
    /// its task's age is red
    pub stale: bool,
    pub flag: Option<Flag>,
    /// on a task's last line, its risk: the image board puts its chip at its end (the
    /// text list has it on the task's row)
    pub risk: Option<String>,
    /// it's the task's stat line ("14 files, +612 −148 · Not toured yet"): facts, which
    /// the image board sets apart from the words over it
    pub stat: bool,
}

/// An item of the Later group.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct ItemRow {
    pub id: String,
    pub title: String,
    /// `bullpen later` running on it: the spinner
    pub job: bool,
    /// it asks for a yes (amber), else it's an idea on the list (dim)
    pub asks: bool,
    /// its mark as text: `?` for one that asks, `◇` for an idea
    pub mark: &'static str,
    pub selected: bool,
    /// its drop is asked about (red)
    pub asked: bool,
}

impl Row {
    /// A line under a task or an item: scrolling keeps them with it.
    pub(crate) fn is_detail(&self) -> bool {
        matches!(self, Row::Detail(..) | Row::ItemDetail(..))
    }

    /// The selected task's row, or the selected item's.
    pub(crate) fn is_selected(&self) -> bool {
        match self {
            Row::Task(t) => t.selected,
            Row::Item(i) => i.selected,
            _ => false,
        }
    }

    /// A task's or an item's own row (what a page counts).
    pub(crate) fn is_entry(&self) -> bool {
        matches!(self, Row::Task(_) | Row::Item(_))
    }
}

/// The list's rows, and the width of its ID column: IDs share a column as wide as the
/// longest, so titles and details line up.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct Rows {
    pub rows: Vec<Row>,
    pub idw: usize,
}

/// How many lines of detail the selected task gets on the text board (the image
/// board gives it the lines it has unselected).
const SELECTED_LINES: usize = 8;

/// The rows of a board `w` columns wide (`narrow`: a task's age goes on its detail line
/// in the grouped layout): each group's heading, then its tasks, each (grouped) with
/// its detail lines; the Later group after the open ones, before Closed. With no group
/// to show, what the board says instead.
pub(crate) fn build(app: &App, w: usize, narrow: bool, now: Now) -> Rows {
    build_as(app, w, narrow, now, true)
}

/// The same rows with nothing selected: every task and item as it is when another is.
/// The image look draws the list from these, and the selected one's block over them
/// (image/pieces.rs), so moving the selection doesn't draw the list again.
pub(crate) fn unselected(app: &App, w: usize, narrow: bool, now: Now) -> Rows {
    build_as(app, w, narrow, now, false)
}

fn build_as(app: &App, w: usize, narrow: bool, now: Now, selection: bool) -> Rows {
    let layout = app.cfg.layout;
    let groups = app.groups();
    let cur_id = app.current().filter(|_| selection).map(|t| t.id.clone());
    let is_cur = |t: &Task| Some(&t.id) == cur_id.as_ref();
    let items = app.later.iter().filter(|_| app.later_open).map(|i| width(&i.id));
    let idw = groups.iter().flat_map(|(_, ts)| ts).map(|t| width(&t.id)).chain(items).max().unwrap_or(0);
    let age_on_detail = narrow && layout == Layout::Grouped;
    // from under the ID to the right edge (the image wraps by pixels: detail_lines)
    let detail_w = w.saturating_sub(5);

    let mut rows: Vec<Row> = Vec::new();
    if groups.is_empty() {
        let lines = model::empty_board(&app.tasks, &app.cfg, app.me.as_deref(), app.closed);
        rows.extend(lines.into_iter().enumerate().map(|(j, l)| Row::Empty { text: l, first: j == 0 }));
    }
    let mut later_due = true;
    for (gi, (heading, ts)) in groups.iter().enumerate() {
        if *heading == "Closed" {
            later_due = false;
            rows.extend(later_rows(app, &app.later, !rows.is_empty(), detail_w, selection));
        }
        // the compact layout's group names, dim, sit where the grouped one leaves a blank row
        if gi > 0 && layout == Layout::Grouped {
            rows.push(Row::Gap);
        }
        rows.push(Row::Head { name: heading.to_string(), count: ts.len() });
        for t in ts {
            rows.push(Row::Task(task_row(app, t, is_cur(t), !age_on_detail, now)));
            if layout == Layout::Grouped {
                details(&mut rows, app, t, is_cur(t), age_on_detail, detail_w, now);
            }
        }
    }
    if later_due {
        rows.extend(later_rows(app, &app.later, !rows.is_empty(), detail_w, selection));
    }
    Rows { rows, idw }
}

/// The most lines the image board gives what a task needs from you, unselected.
const ASKED_LINES: usize = 4;

/// And what it gives a task that's yours to accept or close (ready, finished).
const YOURS_LINES: usize = 3;

/// A task's detail lines (the grouped layout's): what its agent is doing now, or
/// what there is to say about it; then its stat line.
pub(crate) fn details(rows: &mut Vec<Row>, app: &App, t: &Task, cur: bool, age_on_detail: bool, detail_w: usize, now: Now) {
    // a working or in-review task: what its agent is doing now, on one line
    let doing = model::doing_now(t, now);
    // what a task needs from you, GitHub said, or the usage limit gets two lines,
    // as does one its coordinator is on (when it comes to you); anything else
    // one; the selected task all it has to say, up to SELECTED_LINES
    // (on the image board what a task needs from you is never cut short of ASKED_LINES:
    // it's what you have to answer, and what's yours to accept or close gets
    // YOURS_LINES, its risk's reason whole; the selected task gets the lines it has
    // unselected, so moving the selection moves nothing else)
    let lines = if cur && !app.image {
        SELECTED_LINES
    } else if app.image && t.flagged() {
        ASKED_LINES
    } else if app.image && doing.is_empty() && loud_detail(app, t) {
        YOURS_LINES
    } else if !doing.is_empty() {
        // (the image board's has two, so a path doesn't take the line from what's said)
        if app.image { 2 } else { 1 }
    } else if t.flagged() || t.is_limited() || t.triage() || t.status == "finished" || !model::pr_phrase(t).is_empty() {
        2
    } else {
        1
    };
    let text = if doing.is_empty() { model::detail_with_auto(t, &app.cfg, now) } else { doing };
    // on a narrow board the age starts the first line, off the title's
    let age = if age_on_detail { model::ago(t, now) } else { String::new() };
    let text = match (age.is_empty(), text.is_empty()) {
        (true, _) => text,
        (false, true) => age.clone(),
        (false, false) => format!("{age} · {text}"),
    };
    let detail = |text: String, lines: usize, loud: bool, lead: usize, stat: bool| {
        Row::Detail(DetailRow { text, lines, loud, lead, selected: cur, stale: stale(t, now), flag: flag(t), risk: None, stat })
    };
    let first = rows.len();
    for (i, (d, most)) in detail_lines(app, &text, detail_w, lines).into_iter().enumerate() {
        if !d.is_empty() {
            let lead = if i == 0 { width(&age) } else { 0 };
            rows.push(detail(d, most, loud_detail(app, t), lead, false));
        }
    }
    let stat = model::stat_line(t);
    if !stat.is_empty() {
        rows.push(detail(stat, 1, false, 0, true));
    }
    let asked = app.pending.as_ref().is_some_and(|(_, tid)| *tid == t.id);
    if let Some(Row::Detail(d)) = rows[first..].last_mut().filter(|_| !asked) {
        d.risk = bullpen_core::risk::shown(t.obj()).map(|(level, _)| level.to_string());
    }
}

/// A detail's text as the rows that show it, `lines` lines at most, each with the
/// lines it takes: the text list's wrapped to `w` columns, a row a line; the image's
/// whole, in one row, which it wraps by pixels to the room its type has.
pub(super) fn detail_lines(app: &App, text: &str, w: usize, lines: usize) -> Vec<(String, usize)> {
    if app.image {
        return vec![(text.to_string(), lines)];
    }
    model::wrap(text, w, lines).into_iter().map(|line| (line, 1)).collect()
}

/// A task's own row. Asked y/n about, it keeps its title, in the question's colour,
/// with "y / n" for the age; without `with_age` (a narrow board's) its age is on its
/// detail line.
pub(crate) fn task_row(app: &App, t: &Task, selected: bool, with_age: bool, now: Now) -> TaskRow {
    let asked = app.pending.as_ref().filter(|(_, tid)| *tid == t.id).map(|(action, _)| model::confirm_risky(*action, t, &app.cfg));
    let age = match asked {
        Some(_) => "y / n".to_string(),
        None if with_age => model::ago(t, now),
        None => String::new(),
    };
    let risk = if asked.is_some() { None } else { bullpen_core::risk::shown(t.obj()).map(|(level, _)| level.to_string()) };
    TaskRow {
        id: t.id.clone(),
        title: model::shown_title(t, &app.cfg, app.me.as_deref()),
        mark: mark(app, t),
        from_backlog: t.is_from_backlog(),
        flag: flag(t),
        age,
        risk,
        stale: stale(t, now),
        selected,
        asked,
        quiet: t.closed() || t.triage() || app.cfg.theirs(t, app.me.as_deref()),
        yours: (t.flagged() || t.status == "ready") && !app.cfg.theirs(t, app.me.as_deref()),
        key: None,
        repo: String::new(),
        dir: String::new(),
    }
}

/// A task's mark: a task with a command running gets the spinner in place of its own.
fn mark(app: &App, t: &Task) -> Mark {
    let st = t.status.clone();
    match app.job(&t.id) {
        Some(_) => Mark::Job,
        None if t.tour_writing() => Mark::Tour(st),
        None if t.is_stuck() => Mark::Stuck(st),
        // one stopped at the usage limit isn't working either: a still mark
        None if t.is_limited() => Mark::Still(st),
        None if st == "working" => Mark::Working,
        None if st == "review" => Mark::Review,
        None => Mark::Still(st),
    }
}

fn flag(t: &Task) -> Option<Flag> {
    t.flagged().then_some(if t.status == "stopped" { Flag::Stopped } else { Flag::Needs })
}

/// Has the task needed you STALE seconds?
fn stale(t: &Task, now: Now) -> bool {
    t.needs_you() && model::seconds_in_status(t, now) >= STALE
}

/// Is a task's detail at normal weight? What you read and act on is (needs you, stopped,
/// ready, finished); background is dim (working, queued, in review, the coordinator's,
/// closed), and so is all of another coordinator's task, as its title is.
fn loud_detail(app: &App, t: &Task) -> bool {
    (t.needs_you() || t.awaits_accept()) && !app.cfg.theirs(t, app.me.as_deref())
}

/// An item's row.
pub(super) fn item_row(app: &App, i: &Item, selection: bool) -> ItemRow {
    ItemRow {
        id: i.id.clone(),
        title: i.title.clone(),
        job: app.job(&i.id).is_some(),
        asks: i.asks(),
        mark: model::item_mark(i),
        selected: selection && app.current_item().is_some_and(|c| c.id == i.id),
        asked: super::bottom::asked_item(app).is_some_and(|a| a.id == i.id),
    }
}
