//! The bounds: an abstract run of a template over upper bounds instead of
//! values (ADR 0044).
//!
//! Every value is an [`Av`]: bounds on its written size, on the items it
//! iterates and on its numeric magnitude, each a [`Lin`] `c + k·A`, where
//! `A` stands for one piece of the app's data (a `TemplateContext` value of
//! any size) and `k` counts copies of it. A loop over app data counts one
//! iteration ("per app element": the app chose how much data to send); a
//! loop over what the template made (`range`, a literal, a `split`)
//! multiplies by its bound. A product of two app terms (`name |
//! replace(from="", to=name)`) has no linear bound and is refused.
//!
//! `set_global` inside a loop carries a value across iterations. The body
//! is run once with each carried variable replaced by a symbol; every
//! carried value must come out as constant + at most one symbol (so the
//! largest of them grows by at most a constant per iteration), and after
//! `n` iterations is bounded by its start + `n` × that constant. Anything
//! that could grow faster (`s ~ s`, a carried value through `replace`) is
//! refused.

use std::collections::{HashMap, HashSet};

use super::parser::{Binary, ComponentDef, Entry, Expr, Kwargs, Lit, Node, Parsed, Unary};
use super::{CheckError, MAX_TEMPLATE_APP_COPIES, MAX_TEMPLATE_STEPS, MAX_TEMPLATE_VALUE_BYTES};

/// Abstract evaluations allowed per check (nested loops evaluate their
/// bodies twice, components per call site): past it the template is too
/// complex to check, and refused.
const MAX_ANALYSIS_WORK: u64 = 1 << 22;
/// Templates and components evaluated inside each other at most: the
/// analysis recurses per level (a 2 MB asset thread), Tera's own
/// component limit is 20.
const MAX_NESTING: usize = 64;
/// Tera's `range` limit.
const RANGE_MAX: u64 = 100_000;
/// A written number, at most (`-9223372036854775808`, a float's digits).
const NUMBER_BYTES: u64 = 24;
/// What HTML escaping makes of one byte, at most (`&quot;`).
const ESCAPE: u64 = 6;
/// Overhead per item of a written list or map (quotes, `, `, `: `).
const ITEM_BYTES: u64 = 8;
const SAT: u64 = u64::MAX;

/// `c + k·A + Σ n·symbol` (saturating). `poison`: a product of symbols,
/// no linear bound.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Lin {
    pub c: u64,
    pub k: u64,
    syms: Vec<(u32, u64)>,
    poison: bool,
}

impl Lin {
    fn new(c: u64) -> Self {
        Lin {
            c,
            ..Lin::default()
        }
    }

    fn app() -> Self {
        Lin {
            k: 1,
            ..Lin::default()
        }
    }

    fn sym(id: u32) -> Self {
        Lin {
            syms: vec![(id, 1)],
            ..Lin::default()
        }
    }

    fn is_const(&self) -> bool {
        self.k == 0 && self.syms.is_empty() && !self.poison
    }

    fn add(&self, other: &Lin) -> Lin {
        let mut syms = self.syms.clone();
        for &(id, n) in &other.syms {
            match syms.iter_mut().find(|(s, _)| *s == id) {
                Some((_, m)) => *m = m.saturating_add(n),
                None => syms.push((id, n)),
            }
        }
        Lin {
            c: self.c.saturating_add(other.c),
            k: self.k.saturating_add(other.k),
            syms,
            poison: self.poison || other.poison,
        }
    }

    fn max(&self, other: &Lin) -> Lin {
        let mut syms = self.syms.clone();
        for &(id, n) in &other.syms {
            match syms.iter_mut().find(|(s, _)| *s == id) {
                Some((_, m)) => *m = (*m).max(n),
                None => syms.push((id, n)),
            }
        }
        Lin {
            c: self.c.max(other.c),
            k: self.k.max(other.k),
            syms,
            poison: self.poison || other.poison,
        }
    }

    fn scale(&self, n: u64) -> Lin {
        Lin {
            c: self.c.saturating_mul(n),
            k: self.k.saturating_mul(n),
            syms: self
                .syms
                .iter()
                .map(|&(id, m)| (id, m.saturating_mul(n)))
                .filter(|&(_, m)| m > 0)
                .collect(),
            poison: self.poison && n > 0,
        }
    }

    /// An iteration count: each app unit is one iteration.
    fn fold(&self) -> Lin {
        Lin {
            c: self.c.saturating_add(self.k),
            k: 0,
            syms: self.syms.clone(),
            poison: self.poison,
        }
    }

    /// Without the symbols in `inner`.
    fn without(&self, inner: &HashSet<u32>) -> Lin {
        Lin {
            syms: self
                .syms
                .iter()
                .copied()
                .filter(|(id, _)| !inner.contains(id))
                .collect(),
            ..self.clone()
        }
    }
}

/// An abstract value.
#[derive(Clone, Debug, PartialEq)]
struct Av {
    /// Its size when written (lists and maps with their punctuation).
    bytes: Lin,
    /// The items a loop over it runs (a string's characters).
    items: Lin,
    /// Its magnitude as a number (a container's: its items' largest).
    num: Lin,
    /// The size of any one item of it (a character of a string; itself
    /// for a scalar).
    elem: Lin,
    /// Written without escaping.
    safe: bool,
}

impl Av {
    fn app() -> Av {
        Av {
            bytes: Lin::app(),
            items: Lin::app(),
            num: Lin::app(),
            elem: Lin::app(),
            safe: false,
        }
    }

    fn scalar(bytes: u64) -> Av {
        Av {
            bytes: Lin::new(bytes),
            items: Lin::default(),
            num: Lin::default(),
            elem: Lin::new(bytes),
            safe: false,
        }
    }

    fn number(num: Lin) -> Av {
        Av {
            num,
            ..Av::scalar(NUMBER_BYTES)
        }
    }

    fn string(bytes: Lin, safe: bool) -> Av {
        Av {
            items: bytes.clone(),
            bytes,
            num: Lin::default(),
            // A character.
            elem: Lin::new(4),
            safe,
        }
    }

    fn lit(lit: &Lit) -> Av {
        match lit {
            Lit::None => Av::scalar(4),
            Lit::Bool => Av::scalar(5),
            Lit::Number(n) => Av::number(Lin::new(*n)),
            Lit::Str(len) => Av::string(Lin::new(*len as u64), false),
            Lit::Array(items) => {
                let mut out = Av::scalar(0);
                for item in items {
                    let item = Av::lit(item);
                    out.bytes = out.bytes.add(&item.bytes.add(&Lin::new(ITEM_BYTES)));
                    out.num = out.num.max(&item.num);
                    out.elem = out.elem.max(&item.bytes);
                }
                out.items = Lin::new(items.len() as u64);
                out
            }
            Lit::Map(entries) => {
                let mut out = Av::scalar(0);
                for (key, value) in entries {
                    let value = Av::lit(value);
                    let entry = value.bytes.add(&Lin::new(ITEM_BYTES + *key as u64));
                    out.bytes = out.bytes.add(&entry);
                    out.num = out.num.max(&value.num);
                    out.elem = out.elem.max(&value.bytes).max(&Lin::new(*key as u64));
                }
                out.items = Lin::new(entries.len() as u64);
                out
            }
        }
    }

    /// Any one item of it (or a map key, an attribute, a slice's item).
    fn element(&self) -> Av {
        Av {
            bytes: self.elem.clone(),
            items: self.elem.clone(),
            num: self.num.clone(),
            elem: self.elem.clone(),
            safe: false,
        }
    }

    fn max(&self, other: &Av) -> Av {
        Av {
            bytes: self.bytes.max(&other.bytes),
            items: self.items.max(&other.items),
            num: self.num.max(&other.num),
            elem: self.elem.max(&other.elem),
            safe: self.safe && other.safe,
        }
    }

    fn written(&self) -> Lin {
        if self.safe {
            self.bytes.clone()
        } else {
            self.bytes.scale(ESCAPE)
        }
    }
}

/// Tera's scopes: loops (innermost last), `set_global` variables, then the
/// context (the app's, or a component's arguments).
#[derive(Clone, Default)]
struct Env {
    loops: Vec<HashMap<String, Av>>,
    globals: HashMap<String, Av>,
    /// A component's arguments (its context); `None`: the app's.
    args: Option<HashMap<String, Av>>,
}

impl Env {
    /// The value seen from scope `level` outward (`loops.len()` =
    /// globals, past it the context).
    fn lookup_from(&self, level: usize, name: &str) -> Av {
        for scope in self.loops[..level.min(self.loops.len())].iter().rev() {
            if let Some(av) = scope.get(name) {
                return av.clone();
            }
        }
        if let Some(av) = self.globals.get(name) {
            return av.clone();
        }
        match &self.args {
            Some(args) => args.get(name).cloned().unwrap_or_else(Av::app),
            None => Av::app(),
        }
    }

    /// The variables held (what a copy costs).
    fn size(&self) -> u64 {
        let loops: usize = self.loops.iter().map(HashMap::len).sum();
        (loops + self.globals.len() + self.args.as_ref().map_or(0, HashMap::len)) as u64
    }

    fn lookup(&self, name: &str) -> Av {
        self.lookup_from(self.loops.len(), name)
    }

    fn set(&mut self, name: &str, av: Av, global: bool) {
        match self.loops.last_mut() {
            Some(scope) if !global => {
                scope.insert(name.to_owned(), av);
            }
            _ => {
                self.globals.insert(name.to_owned(), av);
            }
        }
    }

    /// After an `if`: each variable's larger bound of the two branches
    /// (one a branch didn't set is what it sees below that scope).
    fn join(&mut self, other: &Env) {
        for level in 0..self.loops.len() {
            let keys: HashSet<String> = self.loops[level]
                .keys()
                .chain(other.loops[level].keys())
                .cloned()
                .collect();
            for key in keys {
                let mine = self.loops[level]
                    .get(&key)
                    .cloned()
                    .unwrap_or_else(|| self.lookup_from(level, &key));
                let theirs = other.loops[level]
                    .get(&key)
                    .cloned()
                    .unwrap_or_else(|| other.lookup_from(level, &key));
                self.loops[level].insert(key, mine.max(&theirs));
            }
        }
        let keys: HashSet<String> = self
            .globals
            .keys()
            .chain(other.globals.keys())
            .cloned()
            .collect();
        let context = |env: &Env, key: &str| match &env.args {
            Some(args) => args.get(key).cloned().unwrap_or_else(Av::app),
            None => Av::app(),
        };
        for key in keys {
            let mine = self
                .globals
                .get(&key)
                .cloned()
                .unwrap_or_else(|| context(self, &key));
            let theirs = other
                .globals
                .get(&key)
                .cloned()
                .unwrap_or_else(|| context(other, &key));
            self.globals.insert(key, mine.max(&theirs));
        }
    }
}

/// What one check found: the bounds (read by the tests, which compare
/// them with real renders).
#[derive(Debug)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct Report {
    pub steps: u64,
    pub output: Lin,
}

pub(super) struct Analyzer<'s> {
    templates: &'s HashMap<String, Parsed>,
    components: HashMap<&'s str, &'s ComponentDef>,
    blocks: HashMap<&'s str, HashMap<&'s str, &'s [Node]>>,
    /// In a growth pass: symbols stand for carried values; no steps or
    /// size checks.
    symbolic: bool,
    steps: u64,
    work: u64,
    next_sym: u32,
    /// Templates and components being evaluated (recursion).
    stack: Vec<String>,
    /// The rendered template's lineage, most derived first (`extends`).
    lineage: Vec<&'s str>,
    /// The blocks being evaluated: name and lineage level (`super()`).
    block_stack: Vec<(String, usize)>,
}

fn collect_blocks<'s>(nodes: &'s [Node], out: &mut HashMap<&'s str, &'s [Node]>) {
    for node in nodes {
        match node {
            Node::Block(name, body) => {
                out.insert(name.as_str(), body.as_slice());
                collect_blocks(body, out);
            }
            Node::For {
                body, else_body, ..
            } => {
                collect_blocks(body, out);
                collect_blocks(else_body, out);
            }
            Node::If(_, body, otherwise) => {
                collect_blocks(body, out);
                collect_blocks(otherwise, out);
            }
            Node::BlockSet { body, .. } | Node::FilterSection(_, body) => collect_blocks(body, out),
            _ => {}
        }
    }
}

impl<'s> Analyzer<'s> {
    pub(super) fn new(templates: &'s HashMap<String, Parsed>) -> Self {
        let mut components = HashMap::new();
        let mut blocks = HashMap::new();
        for (name, parsed) in templates {
            for def in &parsed.components {
                components.entry(def.name.as_str()).or_insert(def);
            }
            let mut found = HashMap::new();
            collect_blocks(&parsed.nodes, &mut found);
            blocks.insert(name.as_str(), found);
        }
        Analyzer {
            templates,
            components,
            blocks,
            symbolic: false,
            steps: 0,
            work: 0,
            next_sym: 0,
            stack: Vec::new(),
            lineage: Vec::new(),
            block_stack: Vec::new(),
        }
    }

    pub(super) fn run(mut self, entry: &str) -> Result<Report, CheckError> {
        let mut env = Env::default();
        let output = self.template(entry, &mut env)?;
        self.charge(0)?;
        Ok(Report {
            steps: self.steps,
            output,
        })
    }

    fn charge(&mut self, steps: u64) -> Result<(), CheckError> {
        if !self.symbolic {
            self.steps = self.steps.saturating_add(steps);
            if self.steps > MAX_TEMPLATE_STEPS {
                return Err(CheckError::TooManySteps);
            }
        }
        Ok(())
    }

    fn tick(&mut self) -> Result<(), CheckError> {
        self.work_on(1)
    }

    /// Charges analysis work (`n` units: an evaluation, or a copy of the
    /// scopes).
    fn work_on(&mut self, n: u64) -> Result<(), CheckError> {
        self.work = self.work.saturating_add(n);
        if self.work > MAX_ANALYSIS_WORK {
            return Err(CheckError::TooComplex);
        }
        self.charge(1)
    }

    fn enter(&mut self, entry: String) -> Result<(), CheckError> {
        if self.stack.len() >= MAX_NESTING {
            return Err(CheckError::TooComplex);
        }
        self.stack.push(entry);
        Ok(())
    }

    /// A value the template creates: within the limits.
    fn check(&self, av: &Av) -> Result<(), CheckError> {
        if self.symbolic {
            return Ok(());
        }
        for part in [&av.bytes, &av.items] {
            if part.c > MAX_TEMPLATE_VALUE_BYTES {
                return Err(CheckError::ValueTooLarge);
            }
            if part.k > MAX_TEMPLATE_APP_COPIES {
                return Err(CheckError::TooManyCopies);
            }
        }
        Ok(())
    }

    fn mul(&self, a: &Lin, b: &Lin) -> Result<Lin, CheckError> {
        if a.is_const() {
            return Ok(b.scale(a.c));
        }
        if b.is_const() {
            return Ok(a.scale(b.c));
        }
        let symbolic = !a.syms.is_empty() || !b.syms.is_empty() || a.poison || b.poison;
        if symbolic {
            return Ok(Lin {
                c: a.c.saturating_mul(b.c),
                poison: true,
                ..Lin::default()
            });
        }
        Err(CheckError::AppSquared)
    }

    /// A number product: unbounded is fine (numbers take no memory), so
    /// it saturates instead of failing.
    fn mul_num(&self, a: &Lin, b: &Lin) -> Lin {
        self.mul(a, b).unwrap_or(Lin::new(SAT))
    }

    // ---- templates and statements ------------------------------------

    fn template(&mut self, name: &str, env: &mut Env) -> Result<Lin, CheckError> {
        if self.stack.iter().any(|entry| entry == name) {
            return Err(CheckError::Recursive(name.to_owned()));
        }
        let templates = self.templates;
        let Some(parsed) = templates.get(name) else {
            return Err(CheckError::Unknown(format!("template `{name}`")));
        };
        // The lineage: this template, its parent, … (`extends`).
        let mut lineage = vec![name];
        let mut root = parsed;
        while let Some(parent) = &root.parent {
            if lineage.contains(&parent.as_str()) {
                return Err(CheckError::Recursive(parent.clone()));
            }
            root = templates
                .get(parent)
                .ok_or_else(|| CheckError::Unknown(format!("template `{parent}`")))?;
            let (key, _) = templates.get_key_value(parent).unwrap();
            lineage.push(key.as_str());
        }
        let lineage: Vec<&'s str> = lineage
            .into_iter()
            .map(|n| templates.get_key_value(n).unwrap().0.as_str())
            .collect();
        self.enter(name.to_owned())?;
        let saved_lineage = std::mem::replace(&mut self.lineage, lineage);
        let saved_blocks = std::mem::take(&mut self.block_stack);
        let out = self.nodes(&root.nodes, env);
        self.lineage = saved_lineage;
        self.block_stack = saved_blocks;
        self.stack.pop();
        out
    }

    fn nodes(&mut self, nodes: &'s [Node], env: &mut Env) -> Result<Lin, CheckError> {
        let mut out = Lin::default();
        for node in nodes {
            self.tick()?;
            let written = match node {
                Node::Content(len) => {
                    self.charge(*len as u64 / 64)?;
                    Lin::new(*len as u64)
                }
                Node::Expr(expr) => {
                    let av = self.expr(expr, env)?;
                    let written = av.written();
                    self.charge(written.c / 64)?;
                    written
                }
                Node::Set {
                    name,
                    value,
                    global,
                } => {
                    let av = self.expr(value, env)?;
                    self.check(&av)?;
                    env.set(name, av, *global);
                    Lin::default()
                }
                Node::BlockSet {
                    name,
                    filters,
                    body,
                    global,
                } => {
                    let captured = self.nodes(body, env)?;
                    let mut av = Av::string(captured, true);
                    self.check(&av)?;
                    for (filter, kwargs) in filters {
                        av = self.filter(filter, av, kwargs, env)?;
                    }
                    env.set(name, av, *global);
                    Lin::default()
                }
                Node::FilterSection(filters, body) => {
                    let captured = self.nodes(body, env)?;
                    let mut av = Av::string(captured, true);
                    self.check(&av)?;
                    for (filter, kwargs) in filters {
                        av = self.filter(filter, av, kwargs, env)?;
                    }
                    av.written()
                }
                Node::Include(name) => self.template(name, env)?,
                Node::Block(name, _) => self.block(name, 0, env)?.0,
                Node::For {
                    key,
                    value,
                    target,
                    body,
                    else_body,
                } => self.for_loop(key.as_deref(), value, target, body, else_body, env)?,
                Node::Break | Node::Continue => Lin::default(),
                Node::If(condition, body, otherwise) => {
                    self.expr(condition, env)?;
                    self.work_on(env.size())?;
                    let mut other = env.clone();
                    let yes = self.nodes(body, env)?;
                    let no = self.nodes(otherwise, &mut other)?;
                    env.join(&other);
                    yes.max(&no)
                }
            };
            out = out.add(&written);
        }
        Ok(out)
    }

    /// Block `name`'s most derived definition from lineage level `from`,
    /// evaluated; with the level it was found at.
    fn block(
        &mut self,
        name: &str,
        from: usize,
        env: &mut Env,
    ) -> Result<(Lin, usize), CheckError> {
        let found = self.lineage[from.min(self.lineage.len())..]
            .iter()
            .enumerate()
            .find_map(|(i, template)| {
                self.blocks
                    .get(template)
                    .and_then(|blocks| blocks.get(name))
                    .map(|body| (from + i, *body))
            });
        let Some((level, body)) = found else {
            return Ok((Lin::default(), self.lineage.len()));
        };
        self.block_stack.push((name.to_owned(), level));
        let out = self.nodes(body, env);
        self.block_stack.pop();
        Ok((out?, level))
    }

    /// The `set_global` targets a loop body can assign (through `if`s,
    /// nested loops, captures and includes).
    fn carried(&self, nodes: &'s [Node], out: &mut Vec<String>, seen: &mut Vec<String>) {
        for node in nodes {
            match node {
                Node::Set {
                    name, global: true, ..
                }
                | Node::BlockSet {
                    name, global: true, ..
                } => {
                    if !out.contains(name) {
                        out.push(name.clone());
                    }
                }
                _ => {}
            }
            match node {
                Node::For {
                    body, else_body, ..
                } => {
                    self.carried(body, out, seen);
                    self.carried(else_body, out, seen);
                }
                Node::If(_, body, otherwise) => {
                    self.carried(body, out, seen);
                    self.carried(otherwise, out, seen);
                }
                Node::BlockSet { body, .. }
                | Node::FilterSection(_, body)
                | Node::Block(_, body) => self.carried(body, out, seen),
                Node::Include(name) => {
                    if !seen.contains(name)
                        && let Some(parsed) = self.templates.get(name)
                    {
                        seen.push(name.clone());
                        self.carried(&parsed.nodes, out, seen);
                    }
                }
                _ => {}
            }
        }
    }

    fn loop_scope(
        key: Option<&str>,
        value: &str,
        element: &Av,
        count: &Lin,
    ) -> HashMap<String, Av> {
        let mut scope = HashMap::new();
        scope.insert(value.to_owned(), element.clone());
        if let Some(key) = key {
            scope.insert(key.to_owned(), element.clone());
        }
        for name in ["index", "index0", "length"] {
            scope.insert(format!("__tera_loop_{name}"), Av::number(count.clone()));
        }
        for name in ["first", "last"] {
            scope.insert(format!("__tera_loop_{name}"), Av::scalar(5));
        }
        scope
    }

    fn for_loop(
        &mut self,
        key: Option<&str>,
        value: &str,
        target: &'s Expr,
        body: &'s [Node],
        else_body: &'s [Node],
        env: &mut Env,
    ) -> Result<Lin, CheckError> {
        let target = self.expr(target, env)?;
        let count = target.items.fold();
        let element = target.element();
        let mut carried = Vec::new();
        self.carried(body, &mut carried, &mut Vec::new());
        let finals = if carried.is_empty() {
            Vec::new()
        } else {
            self.growth(&carried, key, value, &element, &count, body, env)?
        };
        for (name, av) in &finals {
            env.globals.insert(name.clone(), av.clone());
        }

        // The worst iteration: carried values at their bounds.
        let before = self.steps;
        env.loops
            .push(Self::loop_scope(key, value, &element, &count));
        let out = self.nodes(body, env);
        env.loops.pop();
        let out = out?;
        for (name, av) in finals {
            env.globals.insert(name, av);
        }
        if !self.symbolic {
            // An iteration costs its own instructions, empty or not.
            let per_iteration = self.steps - before + 2;
            self.steps = before;
            self.charge(per_iteration.saturating_mul(count.c))?;
        }
        let out = self.mul(&count, &out)?;
        let otherwise = self.nodes(else_body, env)?;
        Ok(out.max(&otherwise))
    }

    /// The bounds of the `carried` variables after the loop, or a refusal
    /// when one could grow faster than linearly.
    #[allow(clippy::too_many_arguments)]
    fn growth(
        &mut self,
        carried: &[String],
        key: Option<&str>,
        value: &str,
        element: &Av,
        count: &Lin,
        body: &'s [Node],
        env: &Env,
    ) -> Result<Vec<(String, Av)>, CheckError> {
        self.work_on(env.size())?;
        let mut symbolic = env.clone();
        let mut inner = HashSet::new();
        for name in carried {
            let (bytes, items) = (self.next_sym, self.next_sym + 1);
            self.next_sym += 2;
            inner.insert(bytes);
            inner.insert(items);
            symbolic.globals.insert(
                name.clone(),
                Av {
                    bytes: Lin::sym(bytes),
                    items: Lin::sym(items),
                    num: Lin::new(SAT),
                    elem: Lin::sym(bytes),
                    safe: false,
                },
            );
        }
        symbolic
            .loops
            .push(Self::loop_scope(key, value, element, count));
        let was = std::mem::replace(&mut self.symbolic, true);
        let steps = self.steps;
        let result = self.nodes(body, &mut symbolic);
        self.symbolic = was;
        self.steps = steps;
        result?;

        // Each carried value: constant + at most one carried symbol.
        let mut constant = Lin::default();
        for name in carried {
            let av = symbolic.globals.get(name).cloned().unwrap_or_else(Av::app);
            for part in [&av.bytes, &av.items, &av.elem] {
                let copies: u64 = part
                    .syms
                    .iter()
                    .filter(|(id, _)| inner.contains(id))
                    .map(|(_, n)| *n)
                    .sum();
                if part.poison || copies > 1 {
                    return Err(CheckError::Superlinear(name.clone()));
                }
                constant = constant.max(&part.without(&inner));
            }
        }
        let mut start = Lin::default();
        for name in carried {
            let before = env.lookup(name);
            start = start.max(&before.bytes).max(&before.items);
        }
        let grown = start.add(&self.mul(count, &constant)?);
        Ok(carried
            .iter()
            .map(|name| {
                let bound = Av {
                    bytes: grown.clone(),
                    items: grown.clone(),
                    num: Lin::new(SAT),
                    elem: grown.clone(),
                    safe: false,
                };
                (name.clone(), bound.max(&env.lookup(name)))
            })
            .collect())
    }

    // ---- expressions ---------------------------------------------------

    fn kwargs(
        &mut self,
        kwargs: &'s Kwargs,
        env: &mut Env,
    ) -> Result<HashMap<&'s str, Av>, CheckError> {
        let mut out = HashMap::new();
        for (name, expr) in kwargs {
            let av = self.expr(expr, env)?;
            out.insert(name.as_str(), av);
        }
        Ok(out)
    }

    fn expr(&mut self, expr: &'s Expr, env: &mut Env) -> Result<Av, CheckError> {
        self.tick()?;
        let av = match expr {
            Expr::Lit(lit) => Av::lit(lit),
            Expr::Array(items) => {
                let mut out = Av::scalar(0);
                for (spread, item) in items {
                    let item = self.expr(item, env)?;
                    if *spread {
                        out.bytes = out.bytes.add(&item.bytes);
                        out.items = out.items.add(&item.items);
                        out.num = out.num.max(&item.num);
                        out.elem = out.elem.max(&item.elem);
                    } else {
                        out.bytes = out.bytes.add(&item.bytes).add(&Lin::new(ITEM_BYTES));
                        out.items = out.items.add(&Lin::new(1));
                        out.num = out.num.max(&item.num);
                        out.elem = out.elem.max(&item.bytes);
                    }
                }
                out
            }
            Expr::Map(entries) => self.entries(entries, env)?.0,
            Expr::Var(name) => env.lookup(name),
            Expr::Attr(inner, _) => self.expr(inner, env)?.element(),
            Expr::Item(inner, index) => {
                self.expr(index, env)?;
                self.expr(inner, env)?.element()
            }
            Expr::Slice(inner, parts) => {
                for part in parts {
                    self.expr(part, env)?;
                }
                let mut av = self.expr(inner, env)?;
                av.safe = false;
                av
            }
            Expr::Filter(inner, name, kwargs) => {
                let input = self.expr(inner, env)?;
                self.filter(name, input, kwargs, env)?
            }
            Expr::Test(inner, kwargs) => {
                self.expr(inner, env)?;
                self.kwargs(kwargs, env)?;
                Av::scalar(5)
            }
            Expr::Ternary(condition, yes, no) => {
                self.expr(condition, env)?;
                let yes = self.expr(yes, env)?;
                let no = self.expr(no, env)?;
                yes.max(&no)
            }
            Expr::Comprehension {
                expr,
                key,
                value,
                target,
                condition,
            } => {
                let target = self.expr(target, env)?;
                let count = target.items.fold();
                let before = self.steps;
                env.loops.push(Self::loop_scope(
                    key.as_deref(),
                    value,
                    &target.element(),
                    &count,
                ));
                let result = (|| {
                    if let Some(condition) = condition {
                        self.expr(condition, env)?;
                    }
                    self.expr(expr, env)
                })();
                env.loops.pop();
                let item = result?;
                if !self.symbolic {
                    let per_item = self.steps - before + 2;
                    self.steps = before;
                    self.charge(per_item.saturating_mul(count.c))?;
                }
                Av {
                    bytes: self.mul(&count, &item.bytes.add(&Lin::new(ITEM_BYTES)))?,
                    items: count,
                    num: item.num,
                    elem: item.bytes,
                    safe: false,
                }
            }
            Expr::Component { name, attrs, body } => {
                Av::string(self.component(name, attrs, body, env)?, true)
            }
            Expr::Call(name, kwargs) => self.call(name, kwargs, env)?,
            Expr::Unary(Unary::Not, inner) => {
                self.expr(inner, env)?;
                Av::scalar(5)
            }
            Expr::Unary(Unary::Minus, inner) => Av::number(self.expr(inner, env)?.num),
            Expr::Binary(op, left, right) => {
                let left = self.expr(left, env)?;
                let right = self.expr(right, env)?;
                match op {
                    Binary::StrConcat => {
                        let bytes = left.bytes.add(&right.bytes);
                        Av::string(bytes, false)
                    }
                    Binary::Plus | Binary::Minus => Av::number(left.num.add(&right.num)),
                    Binary::Mul => Av::number(self.mul_num(&left.num, &right.num)),
                    Binary::Mod => Av::number(right.num),
                    Binary::Div | Binary::FloorDiv | Binary::Power => Av::number(Lin::new(SAT)),
                    // `a or b`/`a and b` give an operand.
                    Binary::And | Binary::Or => left.max(&right),
                    Binary::LessThan
                    | Binary::GreaterThan
                    | Binary::LessThanOrEqual
                    | Binary::GreaterThanOrEqual
                    | Binary::Equal
                    | Binary::NotEqual
                    | Binary::In
                    | Binary::Is
                    | Binary::Pipe => Av::scalar(5),
                }
            }
        };
        self.check(&av)?;
        Ok(av)
    }

    /// A map literal or component attributes: the map, and every value
    /// (a spread's items, for arguments it may provide).
    fn entries(
        &mut self,
        entries: &'s [Entry],
        env: &mut Env,
    ) -> Result<(Av, HashMap<&'s str, Av>, Option<Av>), CheckError> {
        let mut out = Av::scalar(0);
        let mut named = HashMap::new();
        let mut spread: Option<Av> = None;
        for entry in entries {
            match entry {
                Entry::Pair(key, value) => {
                    let value = self.expr(value, env)?;
                    out.bytes = out
                        .bytes
                        .add(&value.bytes)
                        .add(&Lin::new(ITEM_BYTES + key.len() as u64));
                    out.items = out.items.add(&Lin::new(1));
                    out.num = out.num.max(&value.num);
                    out.elem = out.elem.max(&value.bytes);
                    named.insert(key.as_str(), value);
                }
                Entry::Spread(value) => {
                    let value = self.expr(value, env)?;
                    out.bytes = out.bytes.add(&value.bytes);
                    out.items = out.items.add(&value.items);
                    out.num = out.num.max(&value.num);
                    out.elem = out.elem.max(&value.elem);
                    let item = value.element();
                    spread = Some(match spread {
                        Some(prev) => prev.max(&item),
                        None => item,
                    });
                }
            }
        }
        Ok((out, named, spread))
    }

    fn component(
        &mut self,
        name: &str,
        attrs: &'s [Entry],
        body: &'s [Node],
        env: &mut Env,
    ) -> Result<Lin, CheckError> {
        let key = format!("component {name}");
        if self.stack.contains(&key) {
            return Err(CheckError::Recursive(name.to_owned()));
        }
        let Some(def) = self.components.get(name).copied() else {
            return Err(CheckError::Unknown(format!("component `{name}`")));
        };
        let (all, named, spread) = self.entries(attrs, env)?;
        let captured = self.nodes(body, env)?;
        let mut args = HashMap::new();
        for param in &def.params {
            let given = named.get(param.name.as_str()).cloned();
            let mut av = match (&given, &param.default) {
                (Some(av), _) => av.clone(),
                (None, Some(default)) => Av::lit(default),
                (None, None) if param.implicit => env.lookup(&param.name),
                (None, None) => Av::scalar(4),
            };
            if param.implicit && given.is_none() {
                av = av.max(&env.lookup(&param.name));
            }
            if let Some(spread) = &spread {
                av = av.max(spread);
            }
            args.insert(param.name.clone(), av);
        }
        if let Some(rest) = &def.rest {
            args.insert(rest.clone(), all);
        }
        args.insert("body".to_owned(), Av::string(captured, true));
        let mut inner = Env {
            args: Some(args),
            ..Env::default()
        };
        self.enter(key)?;
        let saved_lineage = std::mem::take(&mut self.lineage);
        let saved_blocks = std::mem::take(&mut self.block_stack);
        let out = self.nodes(&def.body, &mut inner);
        self.lineage = saved_lineage;
        self.block_stack = saved_blocks;
        self.stack.pop();
        out
    }

    fn call(&mut self, name: &str, kwargs: &'s Kwargs, env: &mut Env) -> Result<Av, CheckError> {
        let args = self.kwargs(kwargs, env)?;
        match name {
            "range" => {
                let zero = Lin::default();
                let end = args.get("end").map_or(&zero, |av| &av.num);
                let start = args.get("start").map_or(&zero, |av| &av.num);
                let span = end.add(start);
                // Tera refuses more than RANGE_MAX items; an app-sized
                // range counts per app element.
                let items = Lin {
                    c: span.c.min(RANGE_MAX),
                    k: span.k.min(1),
                    ..Lin::default()
                };
                Ok(Av {
                    bytes: items.scale(NUMBER_BYTES + ITEM_BYTES),
                    items,
                    num: span,
                    elem: Lin::new(NUMBER_BYTES),
                    safe: false,
                })
            }
            "throw" => Ok(Av::scalar(0)),
            "super" => {
                let Some((block, level)) = self.block_stack.last().cloned() else {
                    return Ok(Av::scalar(0));
                };
                let (out, _) = self.block(&block, level + 1, env)?;
                Ok(Av::string(out, true))
            }
            _ => Err(CheckError::Unknown(format!("function `{name}`"))),
        }
    }

    fn filter(
        &mut self,
        name: &str,
        x: Av,
        kwargs: &'s Kwargs,
        env: &mut Env,
    ) -> Result<Av, CheckError> {
        let args = self.kwargs(kwargs, env)?;
        self.charge(x.bytes.c / 16)?;
        let arg = |name: &str| args.get(name).cloned();
        let bytes_of =
            |name: &str, default: u64| arg(name).map_or(Lin::new(default), |av| av.bytes);
        let av = match name {
            "safe" => Av { safe: true, ..x },
            "escape" => Av::string(x.bytes.scale(ESCAPE), true),
            "escape_html" | "escape_xml" => Av::string(x.bytes.scale(ESCAPE), false),
            // Case mapping grows a character to at most three.
            "upper" | "lower" | "capitalize" | "title" => Av::string(x.bytes.scale(3), x.safe),
            "trim" | "trim_start" | "trim_end" => Av::string(x.bytes, x.safe),
            "truncate" => Av::string(x.bytes.add(&bytes_of("end", 3)), false),
            "wordcount" | "length" => Av::number(x.items),
            "int" | "float" => Av::number(Lin {
                c: SAT,
                k: (x.bytes.k > 0 || x.num.k > 0) as u64,
                ..Lin::default()
            }),
            "abs" | "round" => Av::number(x.num),
            "str" => Av::string(x.bytes, false),
            "default" => x.max(&arg("value").unwrap_or_else(|| Av::scalar(0))),
            "newlines_to_br" => Av::string(x.bytes.scale(4), false),
            "pluralize" => Av::string(bytes_of("singular", 0).max(&bytes_of("plural", 1)), false),
            "replace" => {
                // Every position may match (an empty `from`), each match
                // becomes `to`; escaped when either side is safe.
                let input = x.bytes.scale(ESCAPE);
                let to = bytes_of("to", 0).scale(ESCAPE);
                let matches = input.add(&Lin::new(1));
                Av::string(input.add(&self.mul(&matches, &to)?), false)
            }
            "indent" => {
                let width = arg("width").map_or(Lin::new(4), |av| av.num);
                let width = Lin::new(width.c.saturating_add(width.k).min(1000));
                let lines = x.bytes.add(&Lin::new(1));
                Av::string(x.bytes.add(&self.mul(&lines, &width.scale(4))?), false)
            }
            "split" => {
                let items = x.bytes.add(&Lin::new(2));
                Av {
                    bytes: x.bytes.add(&items.scale(ITEM_BYTES)),
                    items,
                    num: Lin::default(),
                    elem: x.bytes.clone(),
                    safe: false,
                }
            }
            "join" => {
                let sep = bytes_of("sep", 0).scale(ESCAPE);
                Av::string(x.bytes.scale(ESCAPE).add(&self.mul(&x.items, &sep)?), false)
            }
            "reverse" | "sort" | "unique" => Av { safe: false, ..x },
            "first" | "last" | "nth" | "get" => x.element(),
            "values" | "keys" => Av {
                bytes: x.bytes.clone(),
                items: x.items.clone(),
                num: x.num.clone(),
                elem: x.bytes.clone(),
                safe: false,
            },
            "pairs" | "group_by" => Av {
                bytes: x.bytes.scale(2).add(&x.items.scale(3 * ITEM_BYTES)),
                items: x.items.clone(),
                num: x.num.clone(),
                elem: x.bytes.scale(2).add(&Lin::new(3 * ITEM_BYTES)),
                safe: false,
            },
            // bevy_markup's `json` (`template::new_tera`): `\uXXXX` escapes
            // and punctuation.
            "json" => Av::string(x.bytes.scale(ESCAPE).add(&x.items.scale(ITEM_BYTES)), false),
            _ => return Err(CheckError::Unknown(format!("filter `{name}`"))),
        };
        self.check(&av)?;
        Ok(av)
    }
}
