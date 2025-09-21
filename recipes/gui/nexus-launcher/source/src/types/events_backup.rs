//! Unified Event System for nexus-launcher
//!
//! This module provides a single, unified event system that replaces
//! all the old event handling logic with a clean, priority-based architecture.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Event-Prioritäten für die Verarbeitung
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EventPriority {
    Critical = 0, // Input Events (Mouse, Keyboard)
    High = 1,     // Animation Events
    Normal = 2,   // Render Events
    Low = 3,      // Background Events
}

/// Input Event Types
#[derive(Debug, Clone)]
pub enum InputEventType {
    MouseMove { x: i32, y: i32 },
    MouseClick { x: i32, y: i32, button: u8 },
    MouseRelease { x: i32, y: i32, button: u8 },
    KeyPress { key: u32 },
    KeyRelease { key: u32 },
}

/// Animation Event Types
#[derive(Debug, Clone)]
pub enum AnimationEventType {
    Frame,
    TimelineUpdate { timeline_id: String },
    AnimationComplete { timeline_id: String },
}

/// Render Event Types
#[derive(Debug, Clone)]
pub enum RenderEventType {
    Redraw,
    WindowResize { width: u32, height: u32 },
    WindowMove { x: i32, y: i32 },
}

/// Background Event Types
#[derive(Debug, Clone)]
pub enum BackgroundEventType {
    PanelEvent,
    WindowEvent,
    SystemEvent,
}

/// Event Data für verschiedene Event-Typen
#[derive(Debug, Clone)]
pub enum EventData {
    Input(InputEventType),
    Animation(AnimationEventType),
    Render(RenderEventType),
    Background(BackgroundEventType),
}

/// Einheitliche Event-Struktur
#[derive(Debug, Clone)]
pub struct UnifiedEvent {
    pub timestamp: Instant,
    pub priority: EventPriority,
    pub data: EventData,
    pub source: String, // "actionbar", "launcher", "menu", etc.
}

/// Event-Queue-System für alle Komponenten
pub struct UnifiedEventLoop {
    // Event-Queues nach Priorität
    critical_events: VecDeque<UnifiedEvent>,
    high_events: VecDeque<UnifiedEvent>,
    normal_events: VecDeque<UnifiedEvent>,
    low_events: VecDeque<UnifiedEvent>,

    // Rate-Limiting
    last_actbar_time: Instant,
    actbar_count: u32,
    last_panels_time: Instant,
    panels_count: u32,

    // Event-Queue-Monitoring
    total_events_processed: u64,
    last_clear_time: Instant,
}

impl UnifiedEventLoop {
    pub fn new() -> Self {
        Self {
            critical_events: VecDeque::new(),
            high_events: VecDeque::new(),
            normal_events: VecDeque::new(),
            low_events: VecDeque::new(),
            last_actbar_time: Instant::now(),
            actbar_count: 0,
            last_panels_time: Instant::now(),
            panels_count: 0,
            total_events_processed: 0,
            last_clear_time: Instant::now(),
        }
    }

    /// Konvertiert Event zu UnifiedEvent
    pub fn convert_orb_event<T>(&mut self, _ev: &event::Event<T>, _source: &str) -> Option<UnifiedEvent>
    where
        T: PartialEq + Copy + std::fmt::Debug + event::UserData,
    {
        let _timestamp = Instant::now();

        // We need to match on the user_data field, but we can't directly match on the generic type
        // So we'll use a different approach - check the event code or other fields
        // For now, let's return None for all events and let the main.rs handle them directly
        None
    }

    /// Fügt Event zur entsprechenden Queue hinzu
    pub fn add_event(&mut self, event: UnifiedEvent) {
        match event.priority {
            EventPriority::Critical => self.critical_events.push_back(event),
            EventPriority::High => self.high_events.push_back(event),
            EventPriority::Normal => self.normal_events.push_back(event),
            EventPriority::Low => self.low_events.push_back(event),
        }
        self.total_events_processed += 1;
    }

    /// Verarbeitet Events nach Priorität
    pub fn process_events<F>(&mut self, mut handler: F)
    where
        F: FnMut(&UnifiedEvent) -> bool, // true = Event verarbeitet, false = Event ignorieren
    {
        // Critical Priority: Input Events
        while let Some(event) = self.critical_events.pop_front() {
            if !handler(&event) {
                // Event wurde nicht verarbeitet, zurück in Queue
                self.critical_events.push_front(event);
                break;
            }
        }

        // High Priority: Animation Events
        while let Some(event) = self.high_events.pop_front() {
            if !handler(&event) {
                self.high_events.push_front(event);
                break;
            }
        }

        // Normal Priority: Render Events
        while let Some(event) = self.normal_events.pop_front() {
            if !handler(&event) {
                self.normal_events.push_front(event);
                break;
            }
        }

        // Low Priority: Background Events (meist blockiert)
        while let Some(event) = self.low_events.pop_front() {
            if !handler(&event) {
                self.low_events.push_front(event);
                break;
            }
        }
    }

    /// Rate-Limiting für ActBar Events
    fn should_rate_limit_actbar(&mut self) -> bool {
        let now = Instant::now();
        if now.duration_since(self.last_actbar_time) > Duration::from_millis(100) {
            self.actbar_count = 0;
            self.last_actbar_time = now;
        }

        self.actbar_count += 1;
        if self.actbar_count > 10 {
            true
        } else {
            false
        }
    }

    /// Leert alle Event-Queues
    pub fn clear_all_events(&mut self) {
        self.critical_events.clear();
        self.high_events.clear();
        self.normal_events.clear();
        self.low_events.clear();
        self.last_clear_time = Instant::now();
    }

    /// Gibt Gesamtanzahl der Events zurück
    pub fn total_event_count(&self) -> usize {
        self.critical_events.len() +
        self.high_events.len() +
        self.normal_events.len() +
        self.low_events.len()
    }

    /// Gibt Statistiken zurück
    pub fn get_stats(&self) -> (usize, usize, usize, usize) {
        (
            self.critical_events.len(),
            self.high_events.len(),
            self.normal_events.len(),
            self.low_events.len()
        )
    }
}
