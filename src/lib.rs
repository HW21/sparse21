use std::fmt;
use std::usize::MAX;
use std::cmp::{min, max};
use std::ops::{Index, IndexMut};
use std::error::Error;


#[derive(Debug, Copy, Clone, PartialEq, Eq)]
struct Eindex(usize);

// `Entry`s are a type alias for tuples of (row, col, val).
type Entry = (usize, usize, f64);


#[derive(Debug, Copy, Clone)]
enum Axis { ROWS = 0, COLS }

use Axis::*;


impl Axis {
    fn other(&self) -> Axis {
        match self {
            Axis::ROWS => Axis::COLS,
            Axis::COLS => Axis::ROWS,
        }
    }
}

struct AxisPair<T> {
    rows: T,
    cols: T,
}

impl<T> Index<Axis> for AxisPair<T> {
    type Output = T;

    fn index(&self, ax: Axis) -> &Self::Output {
        match ax {
            Axis::ROWS => &self.rows,
            Axis::COLS => &self.cols,
        }
    }
}

impl<T> IndexMut<Axis> for AxisPair<T> {
    fn index_mut(&mut self, ax: Axis) -> &mut Self::Output {
        match ax {
            Axis::ROWS => &mut self.rows,
            Axis::COLS => &mut self.cols,
        }
    }
}

#[derive(PartialEq, Debug, Copy, Clone)]
enum MatrixState { CREATED = 0, FACTORING, FACTORED }

#[derive(Debug)]
struct Element {
    index: Eindex,
    row: usize,
    col: usize,
    val: f64,
    fillin: bool,
    orig: (usize, usize, f64),
    next_in_row: Option<Eindex>,
    next_in_col: Option<Eindex>,
}

impl PartialEq for Element {
    fn eq(&self, other: &Self) -> bool {
        return self.row == other.row &&
            self.col == other.col &&
            self.val == other.val;
    }
}

impl Element {
    fn new(index: Eindex, row: usize, col: usize, val: f64, fillin: bool) -> Element {
        Element {
            index,
            row,
            col,
            val,
            fillin,
            orig: (row, col, val),
            next_in_row: None,
            next_in_col: None,
        }
    }
    fn loc(&self, ax: Axis) -> usize {
        match ax {
            Axis::ROWS => self.row,
            Axis::COLS => self.col,
        }
    }
    fn set_loc(&mut self, ax: Axis, to: usize) {
        match ax {
            Axis::ROWS => self.row = to,
            Axis::COLS => self.col = to,
        }
    }
    fn next(&self, ax: Axis) -> Option<Eindex> {
        match ax {
            Axis::ROWS => self.next_in_row,
            Axis::COLS => self.next_in_col,
        }
    }
    fn set_next(&mut self, ax: Axis, e: Option<Eindex>) {
        match ax {
            Axis::ROWS => self.next_in_row = e,
            Axis::COLS => self.next_in_col = e,
        }
    }
}

struct AxisMapping {
    e2i: Vec<usize>,
    i2e: Vec<usize>,
    history: Vec<(usize, usize)>,
}

impl AxisMapping {
    pub fn new(size: usize) -> AxisMapping {
        AxisMapping {
            e2i: (0..size).collect(),
            i2e: (0..size).collect(),
            history: vec![],
        }
    }
    fn swap_int(&mut self, x: usize, y: usize) {
        // Swap internal indices x and y
        let tmp = self.i2e[x];
        self.i2e[x] = self.i2e[y];
        self.i2e[y] = tmp;
        self.e2i[self.i2e[x]] = x;
        self.e2i[self.i2e[y]] = y;
        self.history.push((x, y));
    }
}

struct AxisData {
    ax: Axis,
    hdrs: Vec<Option<Eindex>>,
    qtys: Vec<usize>,
    markowitz: Vec<usize>,
    mapping: Option<AxisMapping>,
}

impl AxisData {
    fn new(ax: Axis) -> AxisData {
        AxisData {
            ax: ax,
            hdrs: vec![],
            qtys: vec![],
            markowitz: vec![],
            mapping: None,
        }
    }
    fn grow(&mut self, to: usize) {
        if to <= self.hdrs.len() { return; }
        let by = to - self.hdrs.len();
        for _ in 0..by {
            self.hdrs.push(None);
            self.qtys.push(0);
            self.markowitz.push(0);
        }
    }
    fn setup_factoring(&mut self) {
        self.markowitz.copy_from_slice(&self.qtys);
        self.mapping = Some(AxisMapping::new(self.hdrs.len()));
    }
    fn swap(&mut self, x: usize, y: usize) {
        self.hdrs.swap(x, y);
        self.qtys.swap(x, y);
        self.markowitz.swap(x, y);
        if let Some(m) = &mut self.mapping {
            m.swap_int(x, y);
        }
    }
}

pub struct Matrix {
    // Matrix.elements is the owner of all `Element`s.
    // Everything else gets referenced via `Eindex`es.
    state: MatrixState,
    elements: Vec<Element>,
    axes: AxisPair<AxisData>,
    diag: Vec<Option<Eindex>>,
    fillins: Vec<Eindex>,
}

impl Matrix {
    pub fn new() -> Matrix {
        Matrix {
            state: MatrixState::CREATED,
            axes: AxisPair {
                rows: AxisData::new(Axis::ROWS),
                cols: AxisData::new(Axis::COLS),
            },
            diag: vec![],
            elements: vec![],
            fillins: vec![],
        }
    }

    pub fn from_entries(entries: Vec<Entry>) -> Matrix {
        let mut m = Matrix::new();
        for e in entries.iter() {
            m.add_element(e.0, e.1, e.2);
        }
        return m;
    }

    pub fn identity(n: usize) -> Matrix {
        let mut m = Matrix::new();
        for k in 0..n { m.add_element(k, k, 1.0); }
        return m;
    }

    pub fn add_elements(&mut self, elements: Vec<Entry>) {
        for e in elements.iter() {
            self.add_element(e.0, e.1, e.2);
        }
    }

    fn insert(&mut self, e: &mut Element) {
        let mut expanded = false;
        if e.row + 1 > self.num_rows() {
            self.axes[Axis::ROWS].grow(e.row + 1);
            expanded = true;
        }
        if e.col + 1 > self.num_cols() {
            self.axes[Axis::COLS].grow(e.col + 1);
            expanded = true;
        }
        if expanded {
            let new_diag_len = std::cmp::min(self.num_rows(), self.num_cols());
            for _ in 0..new_diag_len - self.diag.len() {
                self.diag.push(None);
            }
        }

        // Insert along each Axis
        self.insert_axis(Axis::COLS, e);
        self.insert_axis(Axis::ROWS, e);

        // Update row & col qtys
        self.axes[Axis::ROWS].qtys[e.row] += 1;
        self.axes[Axis::COLS].qtys[e.col] += 1;
        if self.state == MatrixState::FACTORING {
            self.axes[Axis::ROWS].markowitz[e.row] += 1;
            self.axes[Axis::COLS].markowitz[e.col] += 1;
        }

        // Update our special arrays
        if e.row == e.col { self.diag[e.row] = Some(e.index); }
        if e.fillin { self.fillins.push(e.index); }
    }

    fn insert_axis(&mut self, ax: Axis, e: &mut Element) {
        // Insert Element `e` along Axis `ax`

        let head_ptr = self.axes[ax].hdrs[e.loc(ax)];
        let head_idx = match head_ptr {
            Some(h) => h,
            None => {
                // Adding first element in this row/col
                return self.set_hdr(ax, e.loc(ax), Some(e.index));
            }
        };
        let off_ax = ax.other();
        if self[head_idx].loc(off_ax) > e.loc(off_ax) {
            // `e` is the new first element
            e.set_next(ax, head_ptr);
            return self.set_hdr(ax, e.loc(ax), Some(e.index));
        }

        // `e` comes after at least one Element.  Search for its position.
        let mut prev = head_idx;
        while let Some(next) = self[prev].next(ax) {
            if self[next].loc(off_ax) >= e.loc(off_ax) { break; }
            prev = next;
        }
        // And splice it in-between `prev` and `nxt`
        e.set_next(ax, self[prev].next(ax));
        self[prev].set_next(ax, Some(e.index));
    }

    pub fn add_element(&mut self, row: usize, col: usize, val: f64) {
        self._add_element(row, col, val, false);
    }

    fn add_fillin(&mut self, row: usize, col: usize) -> Eindex {
        return self._add_element(row, col, 0.0, true);
    }

    fn _add_element(&mut self, row: usize, col: usize, val: f64, fillin: bool) -> Eindex {
        // Element creation & insertion, used by `add_fillin` and the public `add_element`.
        let index = Eindex(self.elements.len());
        let mut e = Element::new(index.clone(), row, col, val, fillin);
        self.insert(&mut e);
        self.elements.push(e);
        return index;
    }

    fn hdr(&self, ax: Axis, loc: usize) -> Option<Eindex> { self.axes[ax].hdrs[loc] }
    fn set_hdr(&mut self, ax: Axis, loc: usize, ei: Option<Eindex>) { self.axes[ax].hdrs[loc] = ei; }

    fn get_hdr_option(&self, ax: Axis, index: usize) -> Option<f64> {
        if index >= self.axes[ax].hdrs.len() { return None; }

        let hdr_ptr = self.axes[ax].hdrs[index];
        return match hdr_ptr {
            None => None,
            Some(ei) => Some(self[ei].val),
        };
    }

    fn num_rows(&self) -> usize {
        self.axes[ROWS].hdrs.len()
    }
    fn num_cols(&self) -> usize {
        self.axes[COLS].hdrs.len()
    }
    fn size(&self) -> (usize, usize) {
        (self.num_rows(), self.num_cols())
    }

    pub fn get(&self, row: usize, col: usize) -> Option<f64> {
        // Returns the Element-value at (row, col) if present, or None if not.

        if row >= self.num_rows() { return None; }
        if col >= self.num_cols() { return None; }

        if row == col { // On diagonal; easy access
            return match self.diag[row] {
                None => None,
                Some(d) => Some(self[d].val),
            };
        }

        let mut ep = self.hdr(ROWS, row);
        while let Some(ei) = ep {
            let e = &self[ei];
            if e.col == col { return Some(e.val); } else if e.col > col { return None; }
            ep = e.next_in_row;
        }
        return None;
    }

    fn set_state(&mut self, state: MatrixState) -> Result<(), &'static str> {
        // Make major state transitions
        match state {
            MatrixState::CREATED => Err("Matrix State Error"),
            MatrixState::FACTORING => {
                if self.state == MatrixState::FACTORING { return Ok(()); }
                if self.state == MatrixState::FACTORED { return Err("Already Factored"); }

                self.axes[Axis::ROWS].setup_factoring();
                self.axes[Axis::COLS].setup_factoring();

                self.state = state;
                return Ok(());
            }
            MatrixState::FACTORED => {
                if self.state == MatrixState::FACTORING {
                    self.state = state;
                    return Ok(());
                } else { return Err("Matrix State Error"); }
            }
        }
    }

    fn move_element(&mut self, ax: Axis, idx: Eindex, to: usize) {
        let loc = self[idx].loc(ax);
        if loc == to { return; }
        let off_ax = ax.other();
        let y = self[idx].loc(off_ax);

        if loc < to {
            let br = match self.before_loc(off_ax, y, to, Some(idx)) {
                Some(ei) => ei,
                None => panic!("ERROR"),
            };
            if br != idx {
                let be = self.prev(off_ax, idx, None);
                let nxt = self[idx].next(off_ax);
                match be {
                    None => self.set_hdr(off_ax, y, nxt),
                    Some(be) => self[be].set_next(off_ax, nxt),
                };
                let brn = self[br].next(off_ax);
                self[idx].set_next(off_ax, brn);
                self[br].set_next(off_ax, Some(idx));
            }
        } else {
            let br = self.before_loc(off_ax, y, to, None);
            let be = self.prev(off_ax, idx, None);

            if br != be { // We (may) need some pointer updates
                if let Some(ei) = be {
                    let nxt = self[idx].next(off_ax);
                    self[ei].set_next(off_ax, nxt);
                }
                match br {
                    None => { // New first in row/col
                        let first = self.hdr(off_ax, y);
                        self[idx].set_next(off_ax, first);
                        self.axes[off_ax].hdrs[y] = Some(idx);
                    }
                    Some(br) => {
                        if br != idx { // Splice `idx` in after `br`
                            let nxt = self[br].next(off_ax);
                            self[idx].set_next(off_ax, nxt);
                            self[br].set_next(off_ax, Some(idx));
                        }
                    }
                };
            }
        }

        // Update the moved-Element's location
        self[idx].set_loc(ax, to);

        if loc == y { // If idx was on our diagonal, remove it
            self.diag[loc] = None;
        } else if to == y { // Or if it's now on the diagonal, add it
            self.diag[to] = Some(idx);
        }
    }
    fn exchange_elements(&mut self, ax: Axis, ix: Eindex, iy: Eindex) {
        // Swap two elements `ax` indices.
        // Elements must be in the same off-axis vector,
        // and the first argument `ex` must be the lower-indexed off-axis.
        // E.g. exchange_elements(Axis.rows, ex, ey) exchanges the rows of ex and ey.

        let off_ax = ax.other();
        let off_loc = self[ix].loc(off_ax);

        let bx = self.prev(off_ax, ix, None);
        let by = match self.prev(off_ax, iy, Some(ix)) {
            Some(e) => e,
            None => panic!("ERROR!"),
        };

        let locx = self[ix].loc(ax);
        let locy = self[iy].loc(ax);
        self[iy].set_loc(ax, locx);
        self[ix].set_loc(ax, locy);

        match bx {
            None => {
                // If `ex` is the *first* entry in the column, replace it to our header-list
                self.set_hdr(off_ax, off_loc, Some(iy));
            }
            Some(bxe) => {
                // Otherwise patch ey into bx
                self[bxe].set_next(off_ax, Some(iy));
            }
        }

        if by == ix { // `ex` and `ey` are adjacent
            let tmp = self[iy].next(off_ax);
            self[iy].set_next(off_ax, Some(ix));
            self[ix].set_next(off_ax, tmp);
        } else { // Elements in-between `ex` and `ey`.  Update the last one.
            let xnxt = self[ix].next(off_ax);
            let ynxt = self[iy].next(off_ax);
            self[iy].set_next(off_ax, xnxt);
            self[ix].set_next(off_ax, ynxt);
            self[by].set_next(off_ax, Some(ix));
        }

        // Update our diagonal array, if necessary
        if locx == off_loc {
            self.diag[off_loc] = Some(iy);
        } else if locy == off_loc {
            self.diag[off_loc] = Some(ix);
        }
    }

    fn prev(&self, ax: Axis, idx: Eindex, hint: Option<Eindex>) -> Option<Eindex> {
        // Find the element previous to `idx` along axis `ax`. 
        // If provided, `hint` *must* be before `idx`, or search will fail. 
        let prev: Option<Eindex> = match hint {
            Some(_) => hint,
            None => self.hdr(ax, self[idx].loc(ax)),
        };
        let mut pi: Eindex = match prev {
            None => { return None; }
            Some(pi) if pi == idx => { return None; }
            Some(pi) => pi,
        };
        while let Some(nxt) = self[pi].next(ax) {
            if nxt == idx { break; }
            pi = nxt;
        }
        return Some(pi);
    }
    fn before_loc(&self, ax: Axis, loc: usize, before: usize, hint: Option<Eindex>) -> Option<Eindex> {
        let prev: Option<Eindex> = match hint {
            Some(_) => hint,
            None => self.hdr(ax, loc),
        };
        let off_ax = ax.other();
        let mut pi: Eindex = match prev {
            None => { return None; }
            Some(pi) if self[pi].loc(off_ax) >= before => { return None; }
            Some(pi) => pi,
        };
        while let Some(nxt) = self[pi].next(ax) {
            if self[nxt].loc(off_ax) >= before { break; }
            pi = nxt;
        }
        return Some(pi);
    }

    fn swap(&mut self, ax: Axis, a: usize, b: usize) {
        if a == b { return; }
        let x = min(a, b);
        let y = max(a, b);

        let hdrs = &self.axes[ax].hdrs;
        let mut ix = hdrs[x];
        let mut iy = hdrs[y];
        let off_ax = ax.other();

        loop {
            match (ix, iy) {
                (Some(ex), Some(ey)) => {
                    let ox = self[ex].loc(off_ax);
                    let oy = self[ey].loc(off_ax);
                    if ox < oy {
                        self.move_element(ax, ex, y);
                        ix = self[ex].next(ax);
                    } else if oy < ox {
                        self.move_element(ax, ey, x);
                        iy = self[ey].next(ax);
                    } else {
                        self.exchange_elements(ax, ex, ey);
                        ix = self[ex].next(ax);
                        iy = self[ey].next(ax);
                    }
                }
                (None, Some(ey)) => {
                    self.move_element(ax, ey, x);
                    iy = self[ey].next(ax);
                }
                (Some(ex), None) => {
                    self.move_element(ax, ex, y);
                    ix = self[ex].next(ax);
                }
                (None, None) => { break; }
            }
        }
        // Swap all the relevant pointers & counters
        self.axes[ax].swap(x, y);
    }

    fn lu_factorize(&mut self) {
        // Updates self to S = L + U - I.
        // Diagonal entries are those of U;
        // L has diagonal entries equal to one.

        assert(self.diag.len()).gt(0);
        // FIXME: singularity check
        self.set_state(MatrixState::FACTORING);

        for n in 0..self.diag.len() - 1 {
            let pivot = match self.search_for_pivot(n) {
                None => panic!("SAD!"), // FIXME: return result-like
                Some(p) => p,
            };
            // assert(pivot).ne(None);
            self.swap(ROWS, self[pivot].row, n);
            self.swap(COLS, self[pivot].col, n);
            self.row_col_elim(pivot, n);
        }
        self.set_state(MatrixState::FACTORED);
    }

    fn search_for_pivot(&self, n: usize) -> Option<Eindex> {
        let mut ei = self.markowitz_search_diagonal(n);
        if let Some(_) = ei { return ei; }
        ei = self.markowitz_search_submatrix(n);
        if let Some(_) = ei { return ei; }
        return self.find_max(n);
    }

    fn max_after(&self, ax: Axis, after: Eindex) -> Eindex {
        let mut best = after;
        let mut best_val = self[after].val.abs();
        let mut e = self[after].next(ax);

        while let Some(ei) = e {
            let val = self[ei].val.abs();
            if val > best_val {
                best = ei;
                best_val = val;
            }
            e = self[ei].next(ax);
        }
        return best;
    }

    fn markowitz_product(&self, ei: Eindex) -> usize {
        let e = &self[ei];
        let mr = self[Axis::ROWS].markowitz[e.row];
        let mc = self[Axis::COLS].markowitz[e.col];
        assert(mr).gt(0);
        assert(mc).gt(0);
        return (mr - 1) * (mc - 1);
    }

    fn markowitz_search_diagonal(&self, n: usize) -> Option<Eindex> {
        let REL_THRESHOLD = 1e-3;
        let ABS_THRESHOLD = 0.0;
        let TIES_MULT = 5;

        let mut best_elem = None;
        let mut best_mark = MAX; // Actually use usize::MAX!
        let mut best_ratio = 0.0;
        let mut num_ties = 0;

        for k in n..self.diag.len() {
            let d = match self.diag[k] {
                None => { continue; }
                Some(d) => d,
            };

            // Check whether this element meets our threshold criteria
            let max_in_col = self.max_after(COLS, d);
            let threshold = REL_THRESHOLD * self[max_in_col].val.abs() + ABS_THRESHOLD;
            if self[d].val.abs() < threshold { continue; }

            // If so, compute and compare its Markowitz product to our best
            let mark = self.markowitz_product(d);
            if mark < best_mark {
                num_ties = 0;
                best_elem = self.diag[k];
                best_mark = mark;
                best_ratio = (self[d].val / self[max_in_col].val).abs();
            } else if mark == best_mark {
                num_ties += 1;
                let ratio = (self[d].val / self[max_in_col].val).abs();
                if ratio > best_ratio {
                    best_elem = self.diag[k];
                    best_mark = mark;
                    best_ratio = ratio;
                }
                if num_ties >= best_mark * TIES_MULT { return best_elem; }
            }
        }
        return best_elem;
    }

    fn markowitz_search_submatrix(&self, n: usize) -> Option<Eindex> {
        let REL_THRESHOLD = 1e-3;
        let ABS_THRESHOLD = 0.0;
        let TIES_MULT = 5;

        let mut best_elem = None;
        let mut best_mark = MAX; // Actually use usize::MAX!
        let mut best_ratio = 0.0;
        let mut num_ties = 0;

        for k in n..self.axes[COLS].hdrs.len() {
            let mut e = self.hdr(COLS, n);
            // Advance to a row ≥ n
            while let Some(ei) = e {
                if self[ei].row >= n { break; }
                e = self[ei].next_in_col;
            }
            let ei = match e {
                None => { continue; }
                Some(d) => d,
            };

            // Check whether this element meets our threshold criteria
            let max_in_col = self.max_after(Axis::COLS, ei);
            let threshold = REL_THRESHOLD * self[max_in_col].val.abs() + ABS_THRESHOLD;

            while let Some(ei) = e {
                // If so, compute and compare its Markowitz product to our best
                let mark = self.markowitz_product(ei);
                if mark < best_mark {
                    num_ties = 0;
                    best_elem = e;
                    best_mark = mark;
                    best_ratio = (self[ei].val / self[max_in_col].val).abs();
                } else if mark == best_mark {
                    num_ties += 1;
                    let ratio = (self[ei].val / self[max_in_col].val).abs();
                    if ratio > best_ratio {
                        best_elem = e;
                        best_mark = mark;
                        best_ratio = ratio;
                    }
//                    // FIXME: do we want tie-counting in here?
//                    if num_ties >= best_mark * TIES_MULT { return best_elem; }
                }
                e = self[ei].next_in_col;
            }
        }
        return best_elem;
    }

    fn find_max(&self, n: usize) -> Option<Eindex> {
        // Find the max (abs value) element in sub-matrix of indices ≥ `n`.
        let mut max_elem = None;
        let mut max_val = 0.0;

        // Search each column ≥ n
        for k in n..self.axes[COLS].hdrs.len() {
            let mut e = self.hdr(COLS, k);

            // Advance to a row ≥ n
            while let Some(ei) = e {
                if self[ei].row >= n { break; }
                e = self[ei].next_in_col;
            }
            // And search over remaining elements
            while let Some(ei) = e {
                let val = self[ei].val.abs();
                if val > max_val {
                    max_elem = e;
                    max_val = val;
                }
                e = self[ei].next_in_col;
            }
        }
        return max_elem;
    }

    fn row_col_elim(&mut self, pivot: Eindex, n: usize) {
        let de = match self.diag[n] {
            Some(de) => de,
            None => panic!("FAIL!"),
        };
        let pivot_val = self[pivot].val;
        assert(pivot_val).ne(0.0);

        // Divide elements in the pivot column by the pivot-value
        let mut plower = self[pivot].next_in_col;
        while let Some(ple) = plower {
            self[ple].val /= pivot_val;
            plower = self[ple].next_in_col;
        }

        let mut pupper = self[pivot].next_in_row;
        while let Some(pue) = pupper {
            let pupper_col = self[pue].col;
            plower = self[pivot].next_in_col;
            let mut psub = self[pue].next_in_col;
            while let Some(ple) = plower {

                // Walk `psub` down to the lower pointer
                while let Some(pse) = psub {
                    if self[pse].row >= self[ple].row { break; }
                    psub = self[pse].next_in_col;
                }
                let pse = match psub {
                    None => self.add_fillin(self[ple].row, pupper_col),
                    Some(pse) if self[pse].row > self[ple].row => {
                        self.add_fillin(self[ple].row, pupper_col)
                    }
                    Some(pse) => pse,
                };

                // Update the `psub` element value
                self[pse].val -= self[pue].val * self[ple].val;
                psub = self[pse].next_in_col;
                plower = self[ple].next_in_col;
            }
            self.axes[COLS].markowitz[pupper_col] -= 1;
            pupper = self[pue].next_in_row;
        }
        // Update remaining Markowitz counts
        self.axes[ROWS].markowitz[n] -= 1;
        self.axes[COLS].markowitz[n] -= 1;
        plower = self[pivot].next_in_col;
        while let Some(ple) = plower {
            let plower_row = self[ple].row;
            self.axes[ROWS].markowitz[plower_row] -= 1;
            plower = self[ple].next_in_col;
        }
    }

    pub fn solve(&mut self, rhs: Vec<f64>) -> Vec<f64> {
        if self.state == MatrixState::CREATED { self.lu_factorize(); }
        assert(self.state).eq(MatrixState::FACTORED);

        // Unwind any row-swaps
        let mut c: Vec<f64> = vec![0.0; rhs.len()];
        for k in 0..c.len() {
            c[self.axes[ROWS].mapping.as_ref().unwrap().e2i[k]] = rhs[k];
        }

        // Forward substitution: Lc=b
        for k in 0..self.diag.len() {
            // Walk down each column, update c
            if c[k] == 0.0 { continue; } // No updates to make on this iteration

            // c[d.row] /= d.val

            let mut di = match self.diag[k] {
                Some(di) => di,
                None => panic!("FAIL!"),
            };
            let mut e = self[di].next_in_col;
            while let Some(ei) = e {
                c[self[ei].row] -= c[k] * self[ei].val;
                e = self[ei].next_in_col;
            }
        }

        // Backward substitution: Ux=c
        for k in (0..self.diag.len()).rev() {
            // Walk each row, update c
            let mut di = match self.diag[k] {
                Some(di) => di,
                None => panic!("FAIL!"),
            };
            let mut e = self[di].next_in_row;
            while let Some(ei) = e {
                c[k] -= c[self[ei].col] * self[ei].val;
                e = self[ei].next_in_row;
            }
            c[k] /= self[di].val;
        }

        // Unwind any column-swaps
        let mut soln: Vec<f64> = vec![0.0; c.len()];
        for k in 0..c.len() {
            soln[k] = c[self.axes[COLS].mapping.as_ref().unwrap().e2i[k]];
        }
        return soln;
    }
    fn swap_rows(&mut self, x: usize, y: usize) { self.swap(ROWS, x, y) }
    fn swap_cols(&mut self, x: usize, y: usize) { self.swap(COLS, x, y) }
}

impl Index<Eindex> for Matrix {
    type Output = Element;
    fn index(&self, index: Eindex) -> &Self::Output { &self.elements[index.0] }
}

impl IndexMut<Eindex> for Matrix {
    fn index_mut(&mut self, index: Eindex) -> &mut Self::Output { &mut self.elements[index.0] }
}

impl Index<Axis> for Matrix {
    type Output = AxisData;
    fn index(&self, ax: Axis) -> &Self::Output { &self.axes[ax] }
}

impl IndexMut<Axis> for Matrix {
    fn index_mut(&mut self, ax: Axis) -> &mut Self::Output { &mut self.axes[ax] }
}

#[derive(Debug, Clone)]
struct NonRealNumError;

impl fmt::Display for NonRealNumError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "invalid first item to double")
    }
}

impl Error for NonRealNumError {
    fn description(&self) -> &str {
        "invalid first item to double"
    }

    fn cause(&self) -> Option<&Error> {
        // Generic error, underlying cause isn't tracked.
        None
    }
}

pub struct System {
    mat: Matrix,
    rhs: Vec<f64>,
    title: Option<String>,
    size: usize,
}

use std::path::Path;

impl System {
    fn lu_factorize(&mut self) { self.mat.lu_factorize() }
    pub fn split(mut self) -> (Matrix, Vec<f64>) { (self.mat, self.rhs) }
    pub fn solve(mut self) -> Vec<f64> {
        self.mat.solve(self.rhs)
    }

    pub fn from_file(filename: &Path) -> Result<System, Box<dyn Error>> {
        use std::fs::File;
        use std::io::BufReader;
        use std::io::prelude::*;

        let mut f = File::open(filename).unwrap();
        let mut f = BufReader::new(f);
        let mut buffer = String::new();
        let mut linesize = f.read_line(&mut buffer)?;

        // Convert the first line to a title
        let title = buffer.trim().to_string();

        // Read the size/ number-format line
        buffer.clear();
        f.read_line(&mut buffer).unwrap();
        let size_strs: Vec<String> = buffer.split_whitespace().map(|s| String::from(s)).collect::<Vec<String>>();
        assert(size_strs.len()).eq(2);
        let size = size_strs[0].clone().parse::<usize>().unwrap();
        assert(size).gt(0);
        let num_type_str = size_strs[1].clone();
        if num_type_str != "real" { return Err(NonRealNumError.into()); }

        // Header stuff checks out.  Create our Matrix.
        let mut m = Matrix::new();

        buffer.clear();
        linesize = f.read_line(&mut buffer).unwrap();
        while linesize != 0 {
            let line_split: Vec<String> = buffer.split_whitespace().map(|s| String::from(s)).collect::<Vec<String>>();
            assert(line_split.len()).eq(3);

            let x = line_split[0].clone().parse::<usize>().unwrap();
            let y = line_split[1].clone().parse::<usize>().unwrap();
            let d = line_split[2].clone().parse::<f64>().unwrap();
            assert(x).le(size);
            assert(y).le(size);

            // Alternate "done" syntax: a line of three zeroes
            if (x == 0) && (y == 0) && (d == 0.0) { break; }
            // This is an Entry.  Add it!
            m.add_element(x - 1, y - 1, d);
            // Update for next iter
            buffer.clear();
            linesize = f.read_line(&mut buffer).unwrap();
        }

        // Read the RHS vector, if present
        let mut rhs: Vec<f64> = Vec::new();
        buffer.clear();
        linesize = f.read_line(&mut buffer).unwrap();
        while linesize != 0 {
            rhs.push(buffer.trim().parse::<f64>()?);
            buffer.clear();
            linesize = f.read_line(&mut buffer)?;
        }
        if rhs.len() > 0 {
            assert(rhs.len()).eq(size);
        }

        return Ok(System {
            mat: m,
            rhs: rhs,
            title: None,
            size: size,
        });
    }
}

struct Assert<T> { val: T }

fn assert<T>(val: T) -> Assert<T> { Assert { val: val } }

impl<T> Assert<T> {
    fn raise(&self) { // Breakpoint here
        panic!("Assertion Failed");
    }
}

impl<T: PartialEq> Assert<T> {
    fn eq(&self, other: T) { if self.val != other { self.raise(); } }
    fn ne(&self, other: T) { if self.val == other { self.raise(); } }
}

impl<T: PartialOrd> Assert<T> {
    fn gt(&self, other: T) { if self.val <= other { self.raise(); } }
    fn lt(&self, other: T) { if self.val >= other { self.raise(); } }
    fn ge(&self, other: T) { if self.val < other { self.raise(); } }
    fn le(&self, other: T) { if self.val > other { self.raise(); } }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checkups(m: &Matrix) {
        // Internal consistency tests.  Probably pretty slow.

        check_diagonal(&m);

        let mut next_in_rows: Vec<Eindex> = vec![];
        let mut next_in_cols: Vec<Eindex> = vec![];

        for n in 0..m.axes[COLS].hdrs.len() {
            let mut ep = m.hdr(COLS, n);
            while let Some(ei) = ep {
                assert(m[ei].col).eq(n);

                if let Some(nxt) = m[ei].next_in_col {
                    assert(m[nxt].row).gt(m[ei].row);
                    assert!(!next_in_cols.contains(&nxt));
                    next_in_cols.push(nxt);
                }
                if let Some(nxt) = m[ei].next_in_row {
                    assert(m[nxt].col).gt(m[ei].col);
                    assert!(!next_in_rows.contains(&nxt));
                    next_in_rows.push(nxt);
                }
                ep = m[ei].next_in_col;
            }
        }

        // Add the row/column headers to the "next" vectors
        for ep in m.axes[Axis::COLS].hdrs.iter() {
            if let Some(ei) = ep {
                assert!(!next_in_cols.contains(ei));
                next_in_cols.push(*ei);
            }
        }
        for ep in m.axes[Axis::ROWS].hdrs.iter() {
            if let Some(ei) = ep {
                assert!(!next_in_rows.contains(ei));
                next_in_rows.push(*ei);
            }
        }

        // Check that all elements are included
        assert(next_in_cols.len()).eq(m.elements.len());
        assert(next_in_rows.len()).eq(m.elements.len());
        for n in 0..m.elements.len() {
            assert!(next_in_cols.contains(&Eindex(n)));
            assert!(next_in_rows.contains(&Eindex(n)));
        }
    }

    fn check_diagonal(m: &Matrix) {
        for r in 0..m.diag.len() {
            let eo = m.get(r, r);
            match eo {
                Some(e) => {
                    if let Some(d) = m.diag[r] {
                        assert(m[d].val).eq(e);
                    } else { panic!("FAIL!"); }
                    // FIXME: would prefer something like the previous "same element ID" testing
                    // assert_eq!(e, m[m.diag[r]].val);
//                    assert_eq!(e.index, m.diag[r]);
//                    assert_eq!(e.row, r);
//                    assert_eq!(e.col, r);
                }
                None => assert_eq!(m.diag[r], None),
            }
        }
    }

    #[test]
    fn test_create_element() {
        let e = Element::new(Eindex(0), 0, 0, 1.0, false);
        assert_eq!(e.index.0, 0);
        assert_eq!(e.row, 0);
        assert_eq!(e.col, 0);
        assert_eq!(e.val, 1.0);
        assert_eq!(e.fillin, false);
        assert_eq!(e.next_in_row, None);
        assert_eq!(e.next_in_col, None);
    }

    #[test]
    fn test_create_matrix() {
        println!("test_create_matrix");

        let m = Matrix::new();
        assert_eq!(m.state, MatrixState::CREATED);
        assert_eq!(m.diag, vec![]);
        assert_eq!(m.axes[Axis::ROWS].hdrs, vec![]);
    }

    #[test]
    fn test_add_element() {
        let mut m = Matrix::new();

        m.add_element(0, 0, 1.0);
        assert_eq!(m.num_rows(), 1);
        assert_eq!(m.num_cols(), 1);
        assert_eq!(m.size(), (1, 1));
        assert_eq!(m.diag.len(), 1);

        m.add_element(100, 100, 1.0);
        assert_eq!(m.num_rows(), 101);
        assert_eq!(m.num_cols(), 101);
        assert_eq!(m.size(), (101, 101));
        assert_eq!(m.diag.len(), 101);
    }

    #[test]
    fn test_get() {
        let mut m = Matrix::new();
        m.add_element(0, 0, 1.0);
        assert(m.get(0, 0).unwrap()).eq(1.0);
    }

    #[test]
    fn test_identity() {
        for k in 1..10 {
            // Check identity matrices of each (small) size

            let ik = Matrix::identity(k);

            // Basic size checks
            assert_eq!(ik.num_rows(), k);
            assert_eq!(ik.num_cols(), k);
            assert_eq!(ik.size(), (k, k));
            assert_eq!(ik.elements.len(), k);
            checkups(&ik);

            for v in 0..k {
                // Check each row/ col head is the same element, and this element is on the diagonal
                let ro = ik.get_hdr_option(Axis::ROWS, v).unwrap();
                let co = ik.get_hdr_option(Axis::COLS, v).unwrap();
                let d0 = ik.get(v, v).unwrap();
                assert_eq!(ro, co);
                assert_eq!(ro, d0);
                assert_eq!(co, d0);
            }
        }
    }

    #[test]
    fn test_swap_rows0() {
        let mut m = Matrix::new();

        m.add_element(0, 0, 11.0);
        m.add_element(7, 0, 22.0);
        m.add_element(0, 7, 33.0);
        m.add_element(7, 7, 44.0);

        checkups(&m);
        assert_eq!(m.get(0, 0).unwrap(), 11.0);
        assert_eq!(m.get(7, 0).unwrap(), 22.0);
        assert_eq!(m.get(0, 7).unwrap(), 33.0);
        assert_eq!(m.get(7, 7).unwrap(), 44.0);

        m.set_state(MatrixState::FACTORING).unwrap();
        m.swap_rows(0, 7);

        checkups(&m);
        assert_eq!(m.get(7, 0).unwrap(), 11.0);
        assert_eq!(m.get(0, 0).unwrap(), 22.0);
        assert_eq!(m.get(7, 7).unwrap(), 33.0);
        assert_eq!(m.get(0, 7).unwrap(), 44.0);
    }

    #[test]
    fn test_swap_rows1() {
        let mut m = Matrix::new();

        m.add_element(0, 0, 11.1);
        m.add_element(2, 2, 22.2);

        checkups(&m);
        assert_eq!(m.get(0, 0).unwrap(), 11.1);
        assert_eq!(m.get(2, 2).unwrap(), 22.2);
        assert_eq!(m.get(1, 1), None);

        m.set_state(MatrixState::FACTORING).unwrap();
        m.swap_rows(0, 2);

        checkups(&m);
        assert_eq!(m.get(2, 0).unwrap(), 11.1);
        assert_eq!(m.get(0, 2).unwrap(), 22.2);
        assert_eq!(m.get(1, 1), None);
    }

    #[test]
    fn test_swap_rows2() {
        let mut m = Matrix::new();

        m.add_element(0, 0, 1.0);
        m.add_element(0, 1, 2.0);
        m.add_element(0, 2, 3.0);
        m.add_element(1, 0, 4.0);
        m.add_element(1, 1, 5.0);
        m.add_element(1, 2, 6.0);
        m.add_element(2, 0, 7.0);
        m.add_element(2, 1, 8.0);
        m.add_element(2, 2, 9.0);

        checkups(&m);
        m.set_state(MatrixState::FACTORING).unwrap();
        m.swap_rows(0, 2);

        checkups(&m);
        assert_eq!(m.get(0, 0).unwrap(), 7.0);
        assert_eq!(m.get(2, 0).unwrap(), 1.0);
        // FIXME: check more
    }

    #[test]
    fn test_swap_rows3() {
        let mut m = Matrix::new();
        m.add_element(1, 0, 71.0);
        m.add_element(2, 0, -11.0);
        m.add_element(2, 2, 99.0);

        checkups(&m);
        assert_eq!(m.get(1, 0).unwrap(), 71.0);
        assert_eq!(m.get(2, 0).unwrap(), -11.0);
        assert_eq!(m.get(2, 2).unwrap(), 99.0);

        m.set_state(MatrixState::FACTORING).unwrap();
        m.swap_rows(0, 2);

        checkups(&m);
        assert_eq!(m.get(1, 0).unwrap(), 71.0);
        assert_eq!(m.get(0, 0).unwrap(), -11.0);
        assert_eq!(m.get(0, 2).unwrap(), 99.0);
    }

    #[test]
    fn test_swap_rows4() {
        let mut m = Matrix::new();

        for r in 0..3 {
            for c in 0..3 {
                if r != 0 || c != 1 {
                    m.add_element(r, c, ((r + 1) * (c + 1)) as f64);
                }
            }
        }
        checkups(&m);

        m.set_state(MatrixState::FACTORING).unwrap();
        m.swap_rows(0, 1);

        checkups(&m);

        // FIXME: add some real checks on this
    }

    #[test]
    fn test_row_mappings() {
        let mut m = Matrix::identity(4);
        checkups(&m);

        m.set_state(MatrixState::FACTORING).unwrap();
        m.swap_rows(0, 3);

        checkups(&m);
        assert_eq!(m.axes[Axis::ROWS].mapping.as_ref().unwrap().e2i, vec![3, 1, 2, 0]);
        assert_eq!(m.axes[Axis::ROWS].mapping.as_ref().unwrap().i2e, vec![3, 1, 2, 0]);

        m.swap_rows(0, 2);

        checkups(&m);
        assert_eq!(m.axes[Axis::ROWS].mapping.as_ref().unwrap().e2i, vec![3, 1, 0, 2]);
        assert_eq!(m.axes[Axis::ROWS].mapping.as_ref().unwrap().i2e, vec![2, 1, 3, 0]);
    }

    #[test]
    fn test_lu_id3() {
        let mut m = Matrix::identity(3);
        checkups(&m);

        m.lu_factorize();

        checkups(&m);
        assert_eq!(m.get(0, 0).unwrap(), 1.0);
        assert_eq!(m.get(1, 1).unwrap(), 1.0);
        assert_eq!(m.get(2, 2).unwrap(), 1.0);
    }

    #[test]
    fn test_lu_lower() {
        // Factors a unit lower-diagonal matrix.  Should leave it unchanged.

        let mut m = Matrix::new();
        m.add_element(0, 0, 1.0);
        m.add_element(1, 0, 1.0);
        m.add_element(2, 0, 1.0);
        m.add_element(1, 1, 1.0);
        m.add_element(2, 1, 1.0);
        m.add_element(2, 2, 1.0);

        checkups(&m);
        assert_eq!(m.get(0, 0).unwrap(), 1.0);
        assert_eq!(m.get(1, 0).unwrap(), 1.0);
        assert_eq!(m.get(2, 0).unwrap(), 1.0);
        assert_eq!(m.get(1, 1).unwrap(), 1.0);
        assert_eq!(m.get(2, 1).unwrap(), 1.0);
        assert_eq!(m.get(2, 2).unwrap(), 1.0);

        m.lu_factorize();

        checkups(&m);
        assert_eq!(m.get(0, 0).unwrap(), 1.0);
        assert_eq!(m.get(1, 0).unwrap(), 1.0);
        assert_eq!(m.get(2, 0).unwrap(), 1.0);
        assert_eq!(m.get(1, 1).unwrap(), 1.0);
        assert_eq!(m.get(2, 1).unwrap(), 1.0);
        assert_eq!(m.get(2, 2).unwrap(), 1.0);
    }

    #[test]
    fn test_lu() {
        let mut m = Matrix::from_entries(vec![
            (2, 2, -1.0),
            (2, 1, 5.0),
            (2, 0, 2.0),
            (1, 2, 5.0),
            (1, 1, 2.0),
            (0, 2, 1.0),
            (0, 1, 1.0),
            (0, 0, 1.0),
        ]);

        checkups(&m);
        assert_entries(&m, vec![
            (2, 2, -1.0),
            (2, 1, 5.0),
            (2, 0, 2.0),
            (1, 2, 5.0),
            (1, 1, 2.0),
            (0, 2, 1.0),
            (0, 1, 1.0),
            (0, 0, 1.0),
        ]);

        m.lu_factorize();

        checkups(&m);
    }

    #[test]
    fn test_solve() {
        let mut m = Matrix::from_entries(vec![
            (0, 0, 1.0),
            (0, 1, 1.0),
            (0, 2, 1.0),
            (1, 1, 2.0),
            (1, 2, 5.0),
            (2, 0, 2.0),
            (2, 1, 5.0),
            (2, 2, -1.0),
        ]);
        checkups(&m);
        m.lu_factorize();
        checkups(&m);

        let rhs = vec![6.0, -4.0, 27.0];
        let soln = m.solve(rhs);
        let correct = vec![5.0, 3.0, -2.0];
        for k in 0..soln.len() {
            assert!(isclose(soln[k], correct[k]));
        }
    }

    #[test]
    fn test_solve_id3() {
        let mut m = Matrix::identity(3);
        let soln = m.solve(vec![11.1, 30.3, 99.9]);
        assert_eq!(soln, vec![11.1, 30.3, 99.9]);
    }

    #[test]
    fn test_solve_identity() {
        /// Test that solutions of Ix=b yield x=b
        for s in 1..10 {
            let mut m = Matrix::identity(s);
            let mut rhs: Vec<f64> = vec![];
            for e in 0..s { rhs.push(e as f64); }

            let soln = m.solve(rhs.clone());
            assert_eq!(soln, rhs);
        }
    }

    fn assert_entries(m: &Matrix, entries: Vec<Entry>) {
        for e in entries.iter() {
            assert(m.get(e.0, e.1).unwrap()).eq(e.2);
        }
    }

    fn isclose(a: f64, b: f64) -> bool {
        return (a - b).abs() < 1e-9;
    }
}

