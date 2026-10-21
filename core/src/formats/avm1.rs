//! A Flash 8 script's functions as far as the hand ports need them: the named functions a frame script defines
//! (`DefineFunction` 0x9b, `DefineFunction2` 0x8e), and what a simple one does, worked out by following its actions
//! on a stack of symbolic values (no interpreter: no loops, no arithmetic, no calls into other script).
//!
//! The Beenox menus (`Opt_graph_xbwin.gfx`, `controls_xbwin.gfx`, `profiles_xbwin.gfx`, `Quest_YesNo.gfx`) define
//! their `gcmd*` entry points on the root timeline; most only set a field from their argument
//! (`this.Camera_Menu.Invert_Palette.Item_1.Item_Value.htmlText = str`), call a method with constant arguments, or
//! send an FSCommand. `summarise` turns those into `Op`s a port applies.

use std::collections::HashMap;

/// A function the script defines: its name, its parameters (and the registers they arrive in for a
/// `DefineFunction2`), whether `this` is preloaded into register 1, and its action bytes.
#[derive(Clone, Debug, PartialEq)]
pub struct Function {
    pub name: String,
    pub params: Vec<(String, u8)>,
    /// The registers `this`, `_global`, `_root` were preloaded into (DefineFunction2's flags), if any.
    pub this_reg: Option<u8>,
    pub global_reg: Option<u8>,
    pub root_reg: Option<u8>,
    pub body: Vec<u8>,
    /// The constant pool in force where it was defined.
    pub pool: Vec<String>,
}

fn cstr(data: &[u8], at: usize) -> (String, usize) {
    let end = data[at.min(data.len())..].iter().position(|&b| b == 0).map_or(data.len(), |n| at + n);
    (String::from_utf8_lossy(&data[at.min(end)..end]).into_owned(), end + 1)
}

fn u16_at(data: &[u8], at: usize) -> u16 {
    data.get(at..at + 2).map_or(0, |b| u16::from_le_bytes([b[0], b[1]]))
}

/// Every named function defined at the top level of a script, with the constant pool in force there.
pub fn functions(data: &[u8]) -> Vec<Function> {
    let mut out = Vec::new();
    let mut pool: Vec<String> = Vec::new();
    let mut at = 0;
    while at < data.len() {
        let op = data[at];
        at += 1;
        if op == 0 {
            break;
        }
        if op < 0x80 {
            continue;
        }
        let size = usize::from(u16_at(data, at));
        let body = at + 2;
        at = body + size;
        match op {
            0x88 => {
                pool.clear();
                let mut q = body + 2;
                for _ in 0..u16_at(data, body) {
                    let (s, next) = cstr(data, q);
                    pool.push(s);
                    q = next;
                }
            }
            0x9b => {
                let (name, mut q) = cstr(data, body);
                let n = u16_at(data, q);
                q += 2;
                let mut params = Vec::new();
                for _ in 0..n {
                    let (p, next) = cstr(data, q);
                    params.push((p, 0));
                    q = next;
                }
                let code = usize::from(u16_at(data, q));
                let end = (at + code).min(data.len());
                if !name.is_empty() {
                    out.push(Function { name, params, this_reg: None, global_reg: None, root_reg: None, body: data[at..end].to_vec(), pool: pool.clone() });
                }
                at = end;
            }
            0x8e => {
                let (name, mut q) = cstr(data, body);
                let n = u16_at(data, q);
                let flags = u16_at(data, q + 3);
                q += 5;
                let mut params = Vec::new();
                for _ in 0..n {
                    let reg = data.get(q).copied().unwrap_or(0);
                    let (p, next) = cstr(data, q + 1);
                    params.push((p, reg));
                    q = next;
                }
                let code = usize::from(u16_at(data, q));
                let end = (at + code).min(data.len());
                // [data] the preload flags in register order: this 0x1, arguments 0x4, super 0x10, _root 0x40,
                // _parent 0x80, _global 0x100 (each preloaded one takes the next register from 1).
                let mut reg = 1u8;
                let mut take = |bit: u16| {
                    if flags & bit != 0 {
                        reg += 1;
                        Some(reg - 1)
                    } else {
                        None
                    }
                };
                let this_reg = take(0x1);
                let _arguments = take(0x4);
                let _super = take(0x10);
                let root_reg = take(0x40);
                let _parent = take(0x80);
                let global_reg = take(0x100);
                if !name.is_empty() {
                    out.push(Function { name, params, this_reg, global_reg, root_reg, body: data[at..end].to_vec(), pool: pool.clone() });
                }
                at = end;
            }
            _ => {}
        }
    }
    out
}

/// A symbolic value on the stack.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Str(String),
    Num(f64),
    Bool(bool),
    Null,
    Undefined,
    /// A function parameter, by its index.
    Param(usize),
    /// An object reached from the timeline the function runs on (`this`, `_root` or a bare name), as its path of
    /// member names (`Camera_Menu/Invert_Palette`). `_global` paths start with "_global".
    Path(Vec<String>),
    /// What a call returned, or anything else not followed.
    Unknown,
}

/// What a function does, in order.
#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    /// `path.field = value`.
    Set { path: Vec<String>, field: String, value: Value },
    /// `path.method(args)` (a bare function call has an empty path).
    Call { path: Vec<String>, method: String, args: Vec<Value> },
    /// `getURL(url, target)`: an FSCommand when the url starts with "FSCommand:".
    GetUrl { url: String, target: Value },
}

/// What `f` does, followed as far as straight-line code allows (a branch, a loop or an action not handled ends the
/// summary, with `complete` false).
pub fn summarise(f: &Function) -> (Vec<Op>, bool) {
    let mut ops = Vec::new();
    let mut stack: Vec<Value> = Vec::new();
    let mut regs: HashMap<u8, Value> = HashMap::new();
    if let Some(r) = f.this_reg {
        regs.insert(r, Value::Path(Vec::new()));
    }
    if let Some(r) = f.root_reg {
        regs.insert(r, Value::Path(Vec::new()));
    }
    if let Some(r) = f.global_reg {
        regs.insert(r, Value::Path(vec!["_global".into()]));
    }
    for (i, (_, reg)) in f.params.iter().enumerate() {
        if *reg != 0 {
            regs.insert(*reg, Value::Param(i));
        }
    }
    let names: HashMap<&str, usize> = f.params.iter().enumerate().map(|(i, (n, _))| (n.as_str(), i)).collect();
    let data = &f.body;
    let mut at = 0;
    let pop = |s: &mut Vec<Value>| s.pop().unwrap_or(Value::Undefined);
    let pop_n = |s: &mut Vec<Value>| match s.pop() {
        Some(Value::Num(n)) => n as usize,
        _ => 0,
    };
    while at < data.len() {
        let op = data[at];
        at += 1;
        if op == 0 {
            return (ops, true);
        }
        let (body, size) = if op >= 0x80 {
            let size = usize::from(u16_at(data, at));
            at += 2 + size;
            (at - size, size)
        } else {
            (at, 0)
        };
        match op {
            // push
            0x96 => {
                let mut q = body;
                let end = body + size;
                while q < end {
                    let kind = data[q];
                    q += 1;
                    let v = match kind {
                        0 => {
                            let (s, next) = cstr(data, q);
                            q = next;
                            Value::Str(s)
                        }
                        1 => {
                            let v = f32::from_le_bytes(data[q..q + 4].try_into().unwrap_or([0; 4]));
                            q += 4;
                            Value::Num(v as f64)
                        }
                        2 => Value::Null,
                        3 => Value::Undefined,
                        4 => {
                            let r = data[q];
                            q += 1;
                            regs.get(&r).cloned().unwrap_or(Value::Unknown)
                        }
                        5 => {
                            let b = data[q] != 0;
                            q += 1;
                            Value::Bool(b)
                        }
                        6 => {
                            let b = &data[q..q + 8];
                            let bits = [b[4], b[5], b[6], b[7], b[0], b[1], b[2], b[3]];
                            q += 8;
                            Value::Num(f64::from_le_bytes(bits))
                        }
                        7 => {
                            let v = i32::from_le_bytes(data[q..q + 4].try_into().unwrap_or([0; 4]));
                            q += 4;
                            Value::Num(v as f64)
                        }
                        8 => {
                            let i = usize::from(data[q]);
                            q += 1;
                            f.pool.get(i).cloned().map_or(Value::Unknown, Value::Str)
                        }
                        9 => {
                            let i = usize::from(u16_at(data, q));
                            q += 2;
                            f.pool.get(i).cloned().map_or(Value::Unknown, Value::Str)
                        }
                        _ => return (ops, false),
                    };
                    stack.push(v);
                }
            }
            // pop
            0x17 => {
                stack.pop();
            }
            // getVariable
            0x1c => {
                let v = match pop(&mut stack) {
                    Value::Str(n) if n == "this" || n == "_root" => Value::Path(Vec::new()),
                    Value::Str(n) => match names.get(n.as_str()) {
                        Some(&i) => Value::Param(i),
                        None => Value::Path(vec![n]),
                    },
                    _ => Value::Unknown,
                };
                stack.push(v);
            }
            // getMember
            0x4e => {
                let name = pop(&mut stack);
                let obj = pop(&mut stack);
                stack.push(match (obj, name) {
                    (Value::Path(mut p), Value::Str(n)) => {
                        p.push(n);
                        Value::Path(p)
                    }
                    _ => Value::Unknown,
                });
            }
            // setMember
            0x4f => {
                let value = pop(&mut stack);
                let field = pop(&mut stack);
                let obj = pop(&mut stack);
                if let (Value::Path(path), Value::Str(field)) = (obj, field) {
                    ops.push(Op::Set { path, field, value });
                }
            }
            // callMethod
            0x52 => {
                let method = pop(&mut stack);
                let obj = pop(&mut stack);
                let n = pop_n(&mut stack);
                let args = (0..n).map(|_| pop(&mut stack)).collect();
                if let (Value::Path(path), Value::Str(method)) = (obj, method) {
                    ops.push(Op::Call { path, method, args });
                }
                stack.push(Value::Unknown);
            }
            // callFunction
            0x3d => {
                let name = pop(&mut stack);
                let n = pop_n(&mut stack);
                let args = (0..n).map(|_| pop(&mut stack)).collect();
                if let Value::Str(method) = name {
                    ops.push(Op::Call { path: Vec::new(), method, args });
                }
                stack.push(Value::Unknown);
            }
            // getURL
            0x83 => {
                let (url, q) = cstr(data, body);
                let (target, _) = cstr(data, q);
                ops.push(Op::GetUrl { url, target: Value::Str(target) });
            }
            // getURL2 (the target and url from the stack)
            0x9a => {
                let target = pop(&mut stack);
                let url = pop(&mut stack);
                if let Value::Str(url) = url {
                    ops.push(Op::GetUrl { url, target });
                }
            }
            // storeRegister
            0x87 => {
                let r = data[body];
                if let Some(v) = stack.last() {
                    regs.insert(r, v.clone());
                }
            }
            // trace: drop its argument
            0x26 => {
                stack.pop();
            }
            // add2 of constants and a parameter: kept only when both are strings
            0x47 => {
                let b = pop(&mut stack);
                let a = pop(&mut stack);
                stack.push(match (a, b) {
                    (Value::Str(a), Value::Str(b)) => Value::Str(a + &b),
                    _ => Value::Unknown,
                });
            }
            // Anything else (branches, arithmetic, other calls): stop here.
            _ => return (ops, false),
        }
    }
    (ops, true)
}

/// The functions a movie's root timeline defines, by name.
pub fn root_functions(movie: &super::gfx::Movie) -> HashMap<String, Function> {
    let Some(super::gfx::Character::Sprite(root)) = movie.characters.get(&super::gfx::ROOT) else { return HashMap::new() };
    root.scripts.iter().flat_map(|(_, s)| functions(&s.raw)).map(|f| (f.name.clone(), f)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Needs the install: OptGraph's resolution setter writes its argument to Item_1's value field, and its
    /// ShutDown state change is an FSCommand.
    #[test]
    fn the_opt_graph_setters_summarise_to_a_field_set_and_an_fscommand() {
        let Ok(file) = std::fs::read(r"C:\Games2\UI\Flash\Opt_graph_xbwin.gfx") else { return };
        let movie = super::super::gfx::Movie::parse(&file).unwrap();
        let fs = root_functions(&movie);
        let (ops, complete) = summarise(&fs["gcmdSetResolution"]);
        assert!(complete);
        assert_eq!(
            ops,
            vec![Op::Set {
                path: ["Camera_Menu", "Invert_Palette", "Item_1", "Item_Value"].map(String::from).to_vec(),
                field: "htmlText".into(),
                value: Value::Param(0)
            }]
        );
        let (ops, _) = summarise(&fs["gcmdChangeState_ShutDown"]);
        assert_eq!(ops, vec![Op::GetUrl { url: "FSCommand:fscmdChangeUIState".into(), target: Value::Str("OptGraphMenuScreens, Shutdown".into()) }]);
    }
}
