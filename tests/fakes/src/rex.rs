//! The tests' fake Rex. It logs each call to rex.log in $FAKE_DIR, keeps its blocks,
//! layers and windows in reg.json there (blocks: {id: {label, layer, window}}; layers:
//! {id: state}; windows: [id]; labels: {window: label}; session; sessions: the ones
//! `new` made, [{session_id, label}], which `ls --json` lists), with a file in alive/
//! for each block that's open, and answers from the files the tests put there:
//!
//!   send_fails_<block>      `send` and `send-key` to it fail
//!   kills_tree_<block>      closing it kills the tree of the command that closed it
//!   screen_<block>, screen  what `capture` shows (one pane's, else every pane's)
//!   pid_<block>, prog_<block>  the process, and program, a pane runs (`process`)
//!   status_<block>.json     the record `program_status` gives (else an older Rex)
//!   server_down, server_version, reload_fails, actions.json, focused
//!
//! It stops early (exit 1, reg.json left as it was) where the Python it replaces raised.

use std::path::PathBuf;
use std::process::Command;

use bullpen_fakes::{
    append, broke, dumps, exit_code, exit_msg, fake_dir, finish, loads, print, read_json, read_text, truthy, Res,
};
use serde_json::{json, Map, Value};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    finish(run(&a))
}

fn run(a: &[String]) -> Res<()> {
    let d = fake_dir()?;
    append(&d.join("rex.log"), &dumps(&json!(a)))?;
    let alive = d.join("alive");
    if !alive.is_dir() {
        std::fs::create_dir(&alive)
            .or_else(|e| if alive.is_dir() { Ok(()) } else { Err(e) })
            .map_err(io)?;
    }
    let reg = d.join("reg.json");
    let r = if reg.exists() {
        read_json(&reg)?
    } else {
        json!({"blocks": {}, "layers": {}, "windows": []})
    };
    let mut rex = Rex { d, alive, reg, r };
    rex.answer(a)?;
    rex.save()
}

struct Rex {
    d: PathBuf,
    alive: PathBuf,
    reg: PathBuf,
    r: Value,
}

fn io(e: std::io::Error) -> bullpen_fakes::Fail {
    broke(&e.to_string())
}

/// a[a.index(flag) + 1]
fn opt<'a>(a: &'a [String], flag: &str) -> Res<&'a str> {
    let i = a
        .iter()
        .position(|x| x == flag)
        .ok_or_else(|| broke(&format!("no {flag}")))?;
    a.get(i + 1)
        .map(String::as_str)
        .ok_or_else(|| broke(&format!("nothing after {flag}")))
}

fn arg(a: &[String], i: usize) -> Res<&str> {
    a.get(i).map(String::as_str).ok_or_else(|| broke("too few arguments"))
}

/// v[k], where v must be an object that has k.
fn idx<'a>(v: &'a Value, k: &str) -> Res<&'a Value> {
    v.as_object()
        .and_then(|m| m.get(k))
        .ok_or_else(|| broke(&format!("no {k:?}")))
}

fn idx_mut<'a>(v: &'a mut Value, k: &str) -> Res<&'a mut Value> {
    v.as_object_mut()
        .and_then(|m| m.get_mut(k))
        .ok_or_else(|| broke(&format!("no {k:?}")))
}

fn obj(v: &mut Value) -> Res<&mut Map<String, Value>> {
    v.as_object_mut().ok_or_else(|| broke("not an object"))
}

/// A JSON value used as a key: it must be a string.
fn key(v: &Value) -> Res<&str> {
    v.as_str().ok_or_else(|| broke(&format!("{v} isn't a string")))
}

impl Rex {
    fn save(&self) -> Res<()> {
        std::fs::write(&self.reg, dumps(&self.r)).map_err(io)
    }

    fn file(&self, name: &str) -> PathBuf {
        self.d.join(name)
    }

    fn is_alive(&self, b: &str) -> bool {
        self.alive.join(b).exists()
    }

    fn blocks(&self) -> Res<&Map<String, Value>> {
        idx(&self.r, "blocks")?
            .as_object()
            .ok_or_else(|| broke("blocks isn't an object"))
    }

    fn blocks_mut(&mut self) -> Res<&mut Map<String, Value>> {
        obj(idx_mut(&mut self.r, "blocks")?)
    }

    fn block_mut(&mut self, b: &Value) -> Res<&mut Map<String, Value>> {
        let b = key(b)?.to_string();
        obj(self
            .blocks_mut()?
            .get_mut(&b)
            .ok_or_else(|| broke(&format!("no block {b}")))?)
    }

    fn layers_mut(&mut self) -> Res<&mut Map<String, Value>> {
        obj(idx_mut(&mut self.r, "layers")?)
    }

    fn windows(&self) -> Res<&Vec<Value>> {
        idx(&self.r, "windows")?
            .as_array()
            .ok_or_else(|| broke("windows isn't a list"))
    }

    fn windows_mut(&mut self) -> Res<&mut Vec<Value>> {
        idx_mut(&mut self.r, "windows")?
            .as_array_mut()
            .ok_or_else(|| broke("windows isn't a list"))
    }

    fn new_block(&mut self, label: Value, layer: Value, window: Value) -> Res<String> {
        let b = format!("blk{}", self.blocks()?.len() + 1);
        std::fs::write(self.alive.join(&b), "").map_err(io)?;
        self.blocks_mut()?
            .insert(b.clone(), json!({"label": label, "layer": layer, "window": window}));
        Ok(b)
    }

    fn new_window(&mut self) -> Res<String> {
        let w = format!("w{}", self.windows()?.len() + 1);
        self.windows_mut()?.push(json!(w));
        Ok(w)
    }

    fn new_layer(&mut self) -> Res<String> {
        let lid = format!("lyr{}", self.layers_mut()?.len() + 1);
        self.layers_mut()?.insert(lid.clone(), json!("normal"));
        Ok(lid)
    }

    fn answer(&mut self, a: &[String]) -> Res<()> {
        let first = arg(a, 0)?;
        let two = |x: &str, y: &str| a.len() >= 2 && a[0] == x && a[1] == y;
        if (first == "send" || first == "send-key") && self.file(&format!("send_fails_{}", opt(a, "-b")?)).exists() {
            return Err(exit_msg("rex: couldn't send to the block")); // what the tests make a pane refuse
        }
        if first == "run" {
            let label = label(a)?;
            let w = self.new_window()?;
            let b = self.new_block(label, Value::Null, json!(w))?;
            print(&dumps(
                &json!({"window_id": idx(&self.blocks()?[&b], "window")?, "block_ids": [b]}),
            ));
        } else if first == "new" {
            // a session of its own: its label is kept (sessions), its first pane a block
            let label = arg(a, 1)?.to_string();
            let sid = format!("session:{label}");
            let w = self.new_window()?;
            let b = self.new_block(Value::Null, Value::Null, json!(w))?;
            let sessions = obj(&mut self.r)?.entry("sessions").or_insert(json!([]));
            sessions
                .as_array_mut()
                .ok_or_else(|| broke("sessions isn't a list"))?
                .push(json!({"session_id": sid, "label": label}));
            print(&dumps(
                &json!({"session_id": sid, "initial_windows": [{"window_id": w, "block_ids": [b]}]}),
            ));
        } else if first == "ls" {
            let sessions = self.r.get("sessions").cloned().unwrap_or(json!([]));
            print(&dumps(&json!({"sessions": sessions})));
        } else if first == "split" {
            let anchor = if a.iter().any(|x| x == "-b") {
                self.blocks()?.get(opt(a, "-b")?).cloned()
            } else {
                None
            };
            let label = label(a)?;
            let window = match anchor {
                Some(anchor) if truthy(&anchor) => idx(&anchor, "window")?.clone(),
                _ => json!(self.new_window()?),
            };
            let b = self.new_block(label, Value::Null, window)?;
            print(&dumps(&json!({"block_ids": [b]})));
        } else if two("block", "inspect") {
            return Err(exit_code(if self.is_alive(arg(a, 2)?) { 0 } else { 1 }));
        } else if two("block", "close") {
            let b = arg(a, 2)?;
            let _ = std::fs::remove_file(self.alive.join(b));
            if self.file(&format!("kills_tree_{b}")).exists() {
                self.kill_tree()?;
            }
        } else if first == "capture" {
            let own = self.file(&format!("screen_{}", opt(a, "-b")?)); // one pane's screen, else every pane's
            let screen = if own.exists() { own } else { self.file("screen") };
            print(&if screen.exists() {
                read_text(&screen)?
            } else {
                "line one\nline two\n\n".into()
            });
        } else if two("block", "call") && a.last().map(String::as_str) == Some("process") {
            self.process(opt(a, "-b")?)?;
        } else if two("block", "call") && a.last().map(String::as_str) == Some("program_status") {
            let rec = self.file(&format!("status_{}.json", opt(a, "-b")?)); // the record the tests give a pane
            if !rec.exists() {
                return Err(exit_msg("rex: no such method")); // an older Rex: bullpen goes by the screen
            }
            print(&dumps(&json!({"records": [read_json(&rec)?]})));
        } else if two("server", "status") {
            if self.file("server_down").exists() {
                return Err(exit_msg("rex: no server"));
            }
            let version = self.file("server_version"); // what the tests say the server is; else Rex says nothing
            if version.exists() {
                print(&dumps(&json!({"rex_version": read_text(&version)?})));
            }
        } else if two("config", "reload") && self.file("reload_fails").exists() {
            return Err(exit_msg("rex: no server"));
        } else if first == "actions" {
            let acts = self.file("actions.json"); // the actions the tests say the Rex config defines
            if acts.exists() {
                print(&read_text(&acts)?);
            }
        } else if two("api", "call") {
            self.api(&a[2..])?;
        }
        Ok(())
    }

    /// `block call -b B process`: the process, and program, the tests say pane B runs.
    fn process(&self, b: &str) -> Res<()> {
        let pid = self.file(&format!("pid_{b}")); // a process the tests run on a terminal of their own
        let prog = self.file(&format!("prog_{b}")); // the program the tests say a pane runs
        if !pid.exists() && !prog.exists() {
            return Err(exit_msg("rex: no such block"));
        }
        let mut child = Map::new();
        if pid.exists() {
            let n: i64 = read_text(&pid)?
                .trim()
                .parse()
                .map_err(|_| broke("pid isn't a number"))?;
            child.insert("pid".into(), json!(n));
        }
        if prog.exists() {
            let p = read_text(&prog)?;
            for k in ["path", "invoked_path", "argv0"] {
                child.insert(k.into(), json!(p));
            }
        }
        print(&dumps(&json!({"child": child})));
        Ok(())
    }

    /// Its program is Claude Code, which kills the tree of the command it ran (the one
    /// closing it, here) as it ends: every descendant by parent, new sessions too. `ps`
    /// lists the processes, here and on macOS alike.
    fn kill_tree(&self) -> Res<()> {
        let out = Command::new("ps")
            .args(["-A", "-o", "pid=,ppid="])
            .output()
            .map_err(io)?;
        let mut kids: std::collections::HashMap<i64, Vec<i64>> = Default::default();
        for row in String::from_utf8_lossy(&out.stdout).lines() {
            let nums: Vec<i64> = row
                .split_whitespace()
                .map(|x| x.parse().map_err(|_| broke("bad ps row")))
                .collect::<Res<_>>()?;
            let [pid, ppid] = nums[..] else {
                return Err(broke("bad ps row"));
            };
            kids.entry(ppid).or_default().push(pid);
        }
        let mut tree = vec![std::os::unix::process::parent_id() as i64];
        let mut i = 0;
        while i < tree.len() {
            tree.extend(kids.get(&tree[i]).into_iter().flatten());
            i += 1;
        }
        self.save()?;
        let me = std::process::id() as i64;
        for pid in tree {
            if pid != me {
                // SAFETY: kill(2) on a pid; it fails harmlessly for one that's gone
                unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) };
            }
        }
        Err(exit_code(0))
    }

    /// `api call [-s SESSION] METHOD [ARGS]`
    fn api(&mut self, rest: &[String]) -> Res<()> {
        let sid = self.r.get("session").cloned().unwrap_or(json!("session:1"));
        let mut rest = rest;
        if arg(rest, 0)? == "-s" {
            let s = json!(arg(rest, 1)?);
            if s != sid && s != json!("main") {
                return Err(exit_code(1)); // a session that no longer exists
            }
            rest = &rest[2..];
        }
        let method = arg(rest, 0)?;
        let args = if rest.len() > 1 { loads(&rest[1])? } else { json!({}) };
        match method {
            "session.new_layer" => {
                let lid = self.new_layer()?;
                let label = idx(idx(idx(&args, "layout")?, "block")?, "label")?.clone();
                let window = match args.get("window_id").filter(|w| truthy(w)) {
                    Some(w) => w.clone(),
                    None => match self.windows()?.first() {
                        Some(w) => w.clone(),
                        None => json!(self.new_window()?),
                    },
                };
                let b = self.new_block(label, json!(lid), window)?;
                print(&dumps(&json!({"layer_id": lid, "block_ids": [b]})));
            }
            "session.new_window" => {
                let w = self.new_window()?;
                let label = args.get("window_label").cloned().unwrap_or(Value::Null);
                let root = obj(&mut self.r)?;
                obj(root.entry("labels").or_insert_with(|| json!({})))?.insert(w.clone(), label);
                let block_label = idx(idx(idx(&args, "layout")?, "block")?, "label")?.clone();
                let b = self.new_block(block_label, Value::Null, json!(w))?;
                print(&dumps(&json!({"window_id": w, "block_ids": [b]})));
            }
            "session.new_block" => {
                let b = self.new_block(idx(&args, "label")?.clone(), json!("detached"), Value::Null)?;
                print(&dumps(&json!({"block_id": b})));
            }
            "session.move_block" => {
                self.block_mut(idx(&args, "block_id")?)?;
                let anchor = Value::Object(self.block_mut(idx(&args, "anchor_block_id")?)?.clone());
                // beside a floating block, as in Rex, it joins that block's layer
                let layer = idx(&anchor, "layer")?.clone();
                let layer = if layer == json!("detached") { Value::Null } else { layer };
                let window = idx(&anchor, "window")?.clone();
                let blk = self.block_mut(idx(&args, "block_id")?)?;
                blk.insert("layer".into(), layer);
                blk.insert("window".into(), window);
                print("{}");
            }
            "session.float_block" => {
                let lid = self.new_layer()?;
                self.block_mut(idx(&args, "block_id")?)?
                    .insert("layer".into(), json!(lid));
                print(&dumps(&json!({"layer_id": lid})));
            }
            "session.detach_block" => {
                let blk = self.block_mut(idx(&args, "block_id")?)?;
                blk.insert("layer".into(), json!("detached"));
                blk.insert("window".into(), Value::Null);
                print("{}");
            }
            "session.list_blocks" => print(&dumps(&json!({"blocks": self.list_blocks()?}))),
            "session.close_window" => {
                let w = idx(&args, "window_id")?.clone();
                let windows = self.windows_mut()?;
                let i = windows
                    .iter()
                    .position(|x| *x == w)
                    .ok_or_else(|| broke("no such window"))?;
                windows.remove(i);
                print("{}");
            }
            "session.set_layer_state" => self.set_layer_state(&args)?,
            "session.view" => print(&dumps(&self.view(sid)?)),
            _ => {}
        }
        Ok(())
    }

    fn list_blocks(&self) -> Res<Vec<Value>> {
        let mut out = vec![];
        for (b, info) in self.blocks()? {
            if !self.is_alive(b) {
                continue;
            }
            let layer = idx(info, "layer")?;
            let mut entry = json!({"block_id": b, "label": idx(info, "label")?});
            let m = obj(&mut entry)?;
            if *layer == json!("detached") {
                m.insert("placement".into(), json!("detached"));
            } else {
                m.insert("placement".into(), json!("placed"));
                m.insert("window_id".into(), idx(info, "window")?.clone());
            }
            out.push(entry);
        }
        Ok(out)
    }

    fn set_layer_state(&mut self, args: &Value) -> Res<()> {
        let lid = idx(args, "layer_id")?.clone();
        let blocks = self.blocks()?;
        let mut w = None;
        for info in blocks.values() {
            if *idx(info, "layer")? == lid {
                w = Some(idx(info, "window")?.clone());
                break;
            }
        }
        let w = w.ok_or_else(|| broke("no block in that layer"))?;
        let layers = idx(&self.r, "layers")?;
        let mut up = 0;
        for (b, info) in blocks {
            if *idx(info, "window")? != w || !self.is_alive(b) {
                continue;
            }
            let layer = idx(info, "layer")?;
            if *layer == lid {
                continue;
            }
            if !truthy(layer) || *idx(layers, key(layer)?)? == json!("normal") {
                up += 1;
            }
        }
        let state = idx(args, "state")?.clone();
        if state == json!("minimized") && up == 0 {
            return Err(exit_msg(
                "rex: cannot minimize layer: it has the window's last visible panes",
            ));
        }
        let lid = key(&lid)?.to_string();
        self.layers_mut()?.insert(lid, state);
        print("{}");
        Ok(())
    }

    fn view(&self, sid: Value) -> Res<Value> {
        let mut windows = vec![];
        let labels = self.r.get("labels");
        for w in self.windows()? {
            let mut layers = vec![json!({"layer_id": format!("tiled-{}", key(w)?), "kind": "tiled", "blocks": []})];
            for (b, info) in self.blocks()? {
                if !self.is_alive(b) || *idx(info, "layer")? == json!("detached") || idx(info, "window")? != w {
                    continue;
                }
                let entry = json!({"block_id": b, "label": idx(info, "label")?});
                let layer = idx(info, "layer")?;
                let i = if truthy(layer) {
                    match layers.iter().position(|l| l["layer_id"] == *layer) {
                        Some(i) => i,
                        None => {
                            let state = idx(idx(&self.r, "layers")?, key(layer)?)?;
                            layers.push(json!({"layer_id": layer, "kind": "floating", "state": state, "blocks": []}));
                            layers.len() - 1
                        }
                    }
                } else {
                    0
                };
                layers[i]["blocks"].as_array_mut().expect("a list").push(entry);
            }
            let mut window = json!({"window_id": w, "layers": layers});
            if let Some(label) = labels
                .and_then(|l| l.get(key(w).unwrap_or_default()))
                .filter(|l| truthy(l))
            {
                obj(&mut window)?.insert("label".into(), label.clone());
            }
            windows.push(window);
        }
        let mut view = json!({"session_id": sid, "label": "main", "windows": windows});
        let focused = self.file("focused"); // the block the tests say has focus, in the active window
        if focused.exists() {
            let b = read_text(&focused)?;
            let blk = self.blocks()?.get(&b).ok_or_else(|| broke(&format!("no block {b}")))?;
            let active = idx(blk, "window")?.clone();
            for w in view["windows"].as_array_mut().expect("a list") {
                if w["window_id"] == active {
                    obj(w)?.insert("focused_block_id".into(), json!(b));
                }
            }
            obj(&mut view)?.insert("active_window_id".into(), active);
        }
        Ok(view)
    }
}

/// The --label value, else none.
fn label(a: &[String]) -> Res<Value> {
    Ok(if a.iter().any(|x| x == "--label") {
        json!(opt(a, "--label")?)
    } else {
        Value::Null
    })
}
