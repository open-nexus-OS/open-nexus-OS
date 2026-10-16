// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The body's statements walked in order (TASK-0215 5c-2 P5d-2): what the pipeline fixes runs
//! here, and the GPU program is the straight line it leaves — no branch, no variable. A function
//! variable holds what was last stored to it, a constant or a computed value, and a load reads it
//! where naga emits the load; an `if` takes the branch its fixed condition picks; a loop runs its
//! body until its fixed condition breaks it, at most [`MAX_ITERATIONS`] times. A branch on a value
//! the program computes is refused.

use naga::{Block, Expression, Handle, Statement};
use nexus_gfx::shader::ir::Scalar;

use super::expr::fixed;
use super::fold::K;
use super::{ty_of, FrontendError, Lowered, Lowering, Result};

/// The most iterations a loop runs while lowering (a blur's taps are tens).
pub(super) const MAX_ITERATIONS: u32 = 1024;

/// How a statement left the walk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Flow {
    Next,
    Break,
    Continue,
    Return,
}

impl Lowering<'_> {
    /// The function variables at entry: their initializers (constants, which naga hoists only
    /// outside loops), else zero — WGSL's default.
    pub(super) fn init_locals(&mut self) -> Result<()> {
        for (var, local) in self.fun.local_variables.iter() {
            let ty = ty_of(self.inner(local.ty))?;
            let value = match local.init {
                Some(init) => self.lower(init)?,
                None => Lowered::Const(K::zero(ty)),
            };
            self.locals.insert(var, value);
        }
        Ok(())
    }

    pub(super) fn block(&mut self, block: &Block) -> Result<Flow> {
        for stmt in block.iter() {
            let flow = self.statement(stmt)?;
            if flow != Flow::Next {
                return Ok(flow);
            }
        }
        Ok(Flow::Next)
    }

    fn statement(&mut self, stmt: &Statement) -> Result<Flow> {
        let unsupported = |what| Err(FrontendError::Unsupported(what));
        match stmt {
            Statement::Emit(range) => {
                self.emit(range.clone())?;
                Ok(Flow::Next)
            }
            Statement::Block(b) => self.block(b),
            Statement::If { condition, accept, reject } => {
                let taken = if self.fixed_condition(*condition)? { accept } else { reject };
                self.block(taken)
            }
            Statement::Loop { body, continuing, break_if } => {
                self.run_loop(body, continuing, *break_if)
            }
            Statement::Break => Ok(Flow::Break),
            Statement::Continue => Ok(Flow::Continue),
            Statement::Store { pointer, value } => {
                self.store(*pointer, *value)?;
                Ok(Flow::Next)
            }
            Statement::Return { value: Some(v) } => {
                self.store_result(*v)?;
                Ok(Flow::Return)
            }
            Statement::Return { value: None } => unsupported("a fragment program without a result"),
            Statement::Kill => unsupported("discard"),
            Statement::Switch { .. } => unsupported("a switch"),
            Statement::Call { .. } => unsupported("a function call"),
            _ => unsupported("a statement outside the subset"),
        }
    }

    /// An `Emit`: the range's expressions are evaluated here. Each is lowered afresh — inside a
    /// loop, with this iteration's variables — and a load of a function variable reads it now,
    /// before a later store changes it.
    fn emit(&mut self, range: naga::Range<Expression>) -> Result<()> {
        for h in range.clone() {
            self.values.remove(&h);
        }
        for h in range {
            if let Expression::Load { pointer } = self.fun.expressions[h] {
                if self.is_variable(pointer) {
                    self.lower(h)?;
                }
            }
        }
        Ok(())
    }

    /// Whether a pointer names a function variable or a part of one.
    fn is_variable(&self, pointer: Handle<Expression>) -> bool {
        match self.fun.expressions[pointer] {
            Expression::LocalVariable(_) => true,
            Expression::AccessIndex { base, .. } | Expression::Access { base, .. } => {
                self.is_variable(base)
            }
            _ => false,
        }
    }

    /// A store to a function variable, or to one lane of a vector one.
    fn store(&mut self, pointer: Handle<Expression>, value: Handle<Expression>) -> Result<()> {
        let v = match self.lower(value)? {
            l @ (Lowered::Value(_) | Lowered::Const(_)) => l,
            _ => return Err(FrontendError::Unsupported("a store of a place or handle")),
        };
        match self.lower(pointer)? {
            Lowered::Local(var) => {
                self.locals.insert(var, v);
            }
            Lowered::LocalLane(var, lane) => {
                let whole = self.local(var)?;
                let with = self.with_lane(whole, lane, v)?;
                self.locals.insert(var, with);
            }
            _ => return Err(FrontendError::Unsupported("a store outside function variables")),
        }
        Ok(())
    }

    /// A vector with one lane replaced: folded when both are constants, else built of lanes.
    fn with_lane(&mut self, whole: Lowered, lane: u8, part: Lowered) -> Result<Lowered> {
        if let (Some(mut w), Some(p)) = (fixed(whole), fixed(part)) {
            w.bits[usize::from(lane)] = p.bits[0];
            return Ok(Lowered::Const(w));
        }
        let (w, p) = (self.as_value(whole)?, self.as_value(part)?);
        let mut parts = Vec::new();
        for i in 0..self.b.ty(w).lanes {
            parts.push(if i == lane { p } else { self.b.extract(w, i) });
        }
        Ok(Lowered::Value(self.b.construct(&parts)))
    }

    /// A loop run here: its body and continuing block once an iteration until a fixed break.
    fn run_loop(
        &mut self,
        body: &Block,
        continuing: &Block,
        break_if: Option<Handle<Expression>>,
    ) -> Result<Flow> {
        for _ in 0..MAX_ITERATIONS {
            match self.block(body)? {
                Flow::Break => return Ok(Flow::Next),
                Flow::Return => return Ok(Flow::Return),
                Flow::Next | Flow::Continue => {}
            }
            if self.block(continuing)? != Flow::Next {
                return Err(FrontendError::Unsupported("a continuing block that leaves the loop"));
            }
            if let Some(c) = break_if {
                if self.fixed_condition(c)? {
                    return Ok(Flow::Next);
                }
            }
        }
        Err(FrontendError::Unsupported("a loop past the iterations a pipeline unrolls"))
    }

    /// A branch's condition: the pipeline must fix it.
    fn fixed_condition(&mut self, h: Handle<Expression>) -> Result<bool> {
        match fixed(self.lower(h)?) {
            Some(k) if k.ty.lanes == 1 && k.ty.scalar == Scalar::Bool => Ok(k.bits[0] != 0),
            _ => Err(FrontendError::Unsupported("a branch on a value the program computes")),
        }
    }
}
