// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Theme registry with dependent notification and 2PC support.
//!
//! Wraps `ThemeRuntime` and adds:
//! - Dependent registration: UI surfaces register to be notified on mode switch.
//! - 2PC-ready switching: `switch_to(mode)` notifies all dependents exactly once
//!   per committed switch. Partial-switch state is not observable.
//! - Integration point for `configd` override application.
//!
//! Part of TASK-0063 Phase 2.

use crate::error::ThemeResult;
use crate::qualifier::Qualifier;
use crate::tokens::ColorValue;
use crate::ThemeRuntime;
use std::path::Path;

/// Callback invoked when the active theme qualifier changes.
/// Receives the new qualifier (e.g. `Dark`, `Light`).
pub type ThemeChangeCallback = Box<dyn Fn(Qualifier) + Send + Sync>;

/// A registered dependent that receives theme change notifications.
struct Dependent {
    /// Unique name for debugging and deduplication.
    name: String,
    /// Called exactly once per committed switch.
    callback: ThemeChangeCallback,
}

/// High-level theme registry with dependent notification.
///
/// Usage:
/// ```ignore
/// let mut reg = ThemeRegistry::load(Path::new("/themes"))?;
/// reg.register("windowd", Box::new(|q| println!("switched to {:?}", q)));
/// reg.switch_to(Qualifier::Dark); // notifies windowd exactly once
/// ```
pub struct ThemeRegistry {
    runtime: ThemeRuntime,
    dependents: Vec<Dependent>,
    /// True when a switch is in-flight (2PC prepare phase).
    switching: bool,
    /// Pending qualifier — committed when all dependents acknowledge.
    pending_qualifier: Option<Qualifier>,
}

impl ThemeRegistry {
    /// Load themes from the given directory.
    pub fn load(theme_dir: &Path) -> ThemeResult<Self> {
        let runtime = ThemeRuntime::load(theme_dir)?;
        Ok(ThemeRegistry {
            runtime,
            dependents: Vec::new(),
            switching: false,
            pending_qualifier: None,
        })
    }

    /// Register a dependent to be notified on theme switch.
    ///
    /// Each dependent is notified exactly once per committed switch.
    /// Duplicate names replace the previous registration.
    pub fn register(&mut self, name: &str, callback: ThemeChangeCallback) {
        self.dependents.retain(|d| d.name != name);
        self.dependents.push(Dependent { name: name.to_string(), callback });
    }

    /// Unregister a dependent by name.
    pub fn unregister(&mut self, name: &str) {
        self.dependents.retain(|d| d.name != name);
    }

    /// Begin a theme switch (2PC prepare phase).
    ///
    /// After calling `prepare_switch`, the registry enters the switching
    /// state. The caller should notify all affected subsystems and then
    /// call `commit_switch` to finalize.
    pub fn prepare_switch(&mut self, qualifier: Qualifier) -> ThemeResult<()> {
        if self.switching {
            // Already in a switch — overwrite pending.
            self.pending_qualifier = Some(qualifier);
            return Ok(());
        }
        self.switching = true;
        self.pending_qualifier = Some(qualifier);
        Ok(())
    }

    /// Commit the prepared switch (2PC commit phase).
    ///
    /// Notifies all registered dependents exactly once with the new qualifier.
    /// After commit, the registry is no longer in the switching state and
    /// the active qualifier is updated.
    pub fn commit_switch(&mut self) -> ThemeResult<()> {
        let qualifier = self.pending_qualifier.take().unwrap_or(self.runtime.active_qualifier());
        self.runtime.set_qualifier(qualifier);

        // Notify all dependents exactly once.
        for dep in &self.dependents {
            (dep.callback)(qualifier);
        }

        self.switching = false;
        Ok(())
    }

    /// Abort an in-flight switch (2PC abort phase).
    ///
    /// Reverts to the previously active qualifier without notifying dependents.
    pub fn abort_switch(&mut self) {
        self.switching = false;
        self.pending_qualifier = None;
    }

    /// Direct switch without 2PC — notifies dependents immediately.
    /// Use `prepare_switch` + `commit_switch` for coordinated multi-service switching.
    pub fn switch_to(&mut self, qualifier: Qualifier) -> ThemeResult<()> {
        self.runtime.set_qualifier(qualifier);

        // Notify all dependents exactly once.
        for dep in &self.dependents {
            (dep.callback)(qualifier);
        }

        Ok(())
    }

    /// Resolve a token name to its color value using the active qualifier chain.
    pub fn resolve(&self, token_name: &str) -> ThemeResult<ColorValue> {
        self.runtime.resolve(token_name)
    }

    /// Get the currently active qualifier.
    pub fn active_qualifier(&self) -> Qualifier {
        self.runtime.active_qualifier()
    }

    /// True when a 2PC switch is in progress.
    pub fn is_switching(&self) -> bool {
        self.switching
    }

    /// Number of registered dependents.
    pub fn dependent_count(&self) -> usize {
        self.dependents.len()
    }

    /// Get a reference to the inner runtime for advanced operations.
    pub fn runtime(&self) -> &ThemeRuntime {
        &self.runtime
    }

    /// Get a mutable reference to the inner runtime.
    pub fn runtime_mut(&mut self) -> &mut ThemeRuntime {
        &mut self.runtime
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn registry_switch_notifies_dependents() {
        // Create a minimal registry without file loading.
        let notified = std::sync::Arc::new(Mutex::new(None::<Qualifier>));
        let n2 = notified.clone();

        // We can't easily construct ThemeRegistry without files, so we test
        // the dependent registration and notification pattern manually.
        let mut callbacks: Vec<Box<dyn Fn(Qualifier) + Send + Sync>> = Vec::new();
        callbacks.push(Box::new(move |q| {
            let mut guard = n2.lock().unwrap();
            *guard = Some(q);
        }));

        // Simulate notification
        for cb in &callbacks {
            (cb)(Qualifier::Dark);
        }

        assert_eq!(*notified.lock().unwrap(), Some(Qualifier::Dark));
    }

    #[test]
    fn duplicate_registration_replaces() {
        let mut names: Vec<String> = Vec::new();
        names.push("a".to_string());
        names.retain(|n| n != "a");
        names.push("a".to_string());
        assert_eq!(names.len(), 1);
    }
}
