// open nexus OS — Component API Reference
// Concatenated .d.ts files from every component group.
// These are TypeScript type declarations describing each component's props (variants, sizes, states, callbacks).
// Use them as the exact contract to reimplement in your own component layer.



/* ============================== CONTROLS ============================== */

/* ---- controls/DatePicker.d.ts ---- */
import React from 'react';

/**
 * DatePicker — glass day·month·year wheel (maps to DatePickerComponent).
 * Composes three WheelPickers; value is a JS Date. Auto-clamps the day when
 * switching to a shorter month. Controlled or uncontrolled.
 *
 * @example
 * <DatePicker defaultValue={new Date(2026, 5, 22)} onChange={setDate} />
 * <DatePicker value={date} onChange={setDate} minYear={2000} maxYear={2030} />
 */
export interface DatePickerProps {
  value?: Date;
  defaultValue?: Date;
  onChange?: (date: Date) => void;
  minYear?: number;
  maxYear?: number;
  /** 12 short month labels (localizable) */
  monthLabels?: string[];
  style?: React.CSSProperties;
}


/* ---- controls/GlassCheckbox.d.ts ---- */
import React from 'react';

/**
 * GlassCheckbox — rounded-square checkbox. Glass outline when off,
 * blue fill with white checkmark when on. Controlled or uncontrolled.
 *
 * @example
 * <GlassCheckbox defaultChecked label="Angebote per E-Mail" />
 * <GlassCheckbox checked={agree} onChange={setAgree} label="Zustimmen" />
 */
export interface GlassCheckboxProps {
  checked?: boolean;
  defaultChecked?: boolean;
  onChange?: (checked: boolean) => void;
  /** Text label to the right of the box */
  label?: string;
  disabled?: boolean;
  style?: React.CSSProperties;
}


/* ---- controls/GlassRadioGroup.d.ts ---- */
import React from 'react';

export interface RadioOption {
  label: string;
  value: string;
}

/**
 * GlassRadioGroup — single-select radio list. Selected row gets a glass
 * highlight and a blue filled dot. Controlled or uncontrolled.
 *
 * @example
 * <GlassRadioGroup options={['Automatisch', 'Hell', 'Dunkel']} defaultValue="Automatisch" />
 * <GlassRadioGroup options={opts} value={mode} onChange={setMode} />
 */
export interface GlassRadioGroupProps {
  /** Options as strings or {label, value} objects */
  options: (string | RadioOption)[];
  value?: string;
  defaultValue?: string;
  onChange?: (value: string) => void;
  disabled?: boolean;
  style?: React.CSSProperties;
}


/* ---- controls/Rating.d.ts ---- */
import React from 'react';

/**
 * Rating — star rating control. Interactive (click/hover) or read-only
 * (supports fractional fill). Filled stars use the accent/warning color.
 *
 * @example
 * <Rating defaultValue={3} onChange={setStars} />
 * <Rating value={4.5} readOnly />
 */
export interface RatingProps {
  value?: number;
  defaultValue?: number;
  max?: number;
  onChange?: (value: number) => void;
  /** Star size in px */
  size?: number;
  /** Display-only; disables interaction and allows fractional value */
  readOnly?: boolean;
  /** Fill color (defaults to --color-warning) */
  color?: string;
  style?: React.CSSProperties;
}


/* ---- controls/Segment.d.ts ---- */
import React from 'react';

export interface SegmentOption {
  label: string;
  value: string;
  /** Optional leading icon node */
  icon?: React.ReactNode;
}

/**
 * Segment — iOS-style segmented control. A glass pill with a sliding
 * white thumb behind the active option. Controlled or uncontrolled.
 *
 * @example
 * <Segment options={['Tag', 'Woche', 'Monat']} defaultValue="Woche" />
 * <Segment options={opts} value={view} onChange={setView} size="lg" />
 */
export interface SegmentProps {
  /** Options as strings or {label, value, icon} objects */
  options: (string | SegmentOption)[];
  /** Controlled selected value */
  value?: string;
  /** Initial value for uncontrolled usage */
  defaultValue?: string;
  /** Fired with the new value on selection */
  onChange?: (value: string) => void;
  /** Control height/typography */
  size?: 'sm' | 'md' | 'lg';
  style?: React.CSSProperties;
}


/* ---- controls/Select.d.ts ---- */
import React from 'react';

export interface SelectOption {
  label: string;
  value: string;
}

/**
 * Select — glass dropdown select. Styled trigger pill with chevron that opens a
 * liquid-glass option panel (same surface as Popover) — never the native OS
 * list. Selected option shows an accent checkmark. Controlled or uncontrolled.
 *
 * @example
 * <Select options={['Deutsch', 'English', 'Français']} defaultValue="Deutsch" />
 * <Select options={opts} value={lang} onChange={setLang} placeholder="Sprache" />
 */
export interface SelectProps {
  options: (string | SelectOption)[];
  value?: string;
  defaultValue?: string;
  onChange?: (value: string) => void;
  /** Shown when no value is selected */
  placeholder?: string;
  disabled?: boolean;
  size?: 'sm' | 'md' | 'lg';
  style?: React.CSSProperties;
}


/* ---- controls/Slider.d.ts ---- */
import React from 'react';

/**
 * Slider — iOS-style range slider (maps to ion-range). Glass track,
 * blue filled portion, draggable white thumb. Controlled or uncontrolled.
 * Supports pointer drag and tap-to-position.
 *
 * @example
 * <Slider defaultValue={60} icon={<Volume1 />} iconEnd={<Volume2 />} />
 * <Slider value={brightness} onChange={setBrightness} min={0} max={100} />
 */
export interface SliderProps {
  /** Controlled value */
  value?: number;
  /** Initial value for uncontrolled usage */
  defaultValue?: number;
  min?: number;
  max?: number;
  step?: number;
  /** Fired continuously while dragging */
  onChange?: (value: number) => void;
  disabled?: boolean;
  /** Icon rendered before the track */
  icon?: React.ReactNode;
  /** Icon rendered after the track */
  iconEnd?: React.ReactNode;
  style?: React.CSSProperties;
}


/* ---- controls/Stepper.d.ts ---- */
import React from 'react';

/**
 * Stepper — compact −/+ numeric stepper in a glass pill.
 * Controlled or uncontrolled, with optional value formatting.
 *
 * @example
 * <Stepper defaultValue={2} min={0} max={9} />
 * <Stepper value={qty} onChange={setQty} formatValue={(v) => `${v}×`} />
 */
export interface StepperProps {
  value?: number;
  defaultValue?: number;
  min?: number;
  max?: number;
  step?: number;
  onChange?: (value: number) => void;
  disabled?: boolean;
  /** Render the displayed value (e.g. add a unit) */
  formatValue?: (value: number) => React.ReactNode;
  style?: React.CSSProperties;
}


/* ---- controls/WheelPicker.d.ts ---- */
import React from 'react';

export interface WheelOption {
  label: string;
  value: string | number;
}

/**
 * WheelPicker — iOS-style snap-scrolling wheel column. Options pass under a
 * centered glass selection band; tap or scroll to choose. Controlled or
 * uncontrolled. Compose several side-by-side for a DatePicker.
 *
 * @example
 * <WheelPicker options={[2023,2024,2025,2026]} defaultValue={2026} />
 * <WheelPicker options={hours} value={h} onChange={setH} width={70} />
 */
export interface WheelPickerProps {
  options: (string | number | WheelOption)[];
  value?: string | number;
  defaultValue?: string | number;
  onChange?: (value: string | number) => void;
  /** Row height in px */
  itemHeight?: number;
  /** Odd number of visible rows */
  visibleCount?: number;
  width?: number;
  style?: React.CSSProperties;
}



/* ============================== CORE ============================== */

/* ---- core/AppIcon.d.ts ---- */
/**
 * AppIcon — adaptive app icon tile with three rendering modes.
 *
 * @example
 * // First-party OS app — shaped icon, no redundant glass backing
 * <AppIcon src="/icons/files.svg" label="Files" variant="native" size="md" />
 *
 * // Sideloaded or web app — glass panel wraps a shapeless icon
 * <AppIcon src="favicon.png" label="Chrome" variant="wrapped" badge={2} />
 *
 * // Stash & special folders — freestanding (icons are pre-composed assets)
 * <AppIcon src="/icons/files.svg" label="Stash" variant="freestanding" />
 * <AppIcon src="/icons/pictures.svg" label="Bilder" variant="freestanding" />
 *
 * // Dock icon (active, no label)
 * <AppIcon src="/icons/settings.svg" size="sm" variant="native" active />
 */
export interface AppIconProps {
  /** Path to the app icon image */
  src: string;
  /** Label rendered below the icon. Ellipsed at icon width + 16px. */
  label?: string;
  /** Size preset. Default: 'md' */
  size?: 'xs' | 'sm' | 'md' | 'lg' | 'xl';
  /**
   * Notification count badge. Rendered **outside** the backing (never clipped).
   * Positioned top-right of the icon area. Hidden when 0 or undefined.
   */
  badge?: number;
  /** Show a 4px active-app indicator dot below the icon (used in the dock). */
  active?: boolean;
  /**
   * Rendering variant. Default: `"native"`.
   *
   * - `"native"` — icon fills the rounded tile directly. Use for first-party OS apps
   *   whose icon already has the correct shape and background. No glass backing.
   * - `"wrapped"` — glass panel backing wraps the icon at 65% fill. Use for sideloaded
   *   apps, web apps, or any icon without a shaped/non-standard background.
   * - `"freestanding"` — completely bare, no backing. Use for special apps like Stash
   *   and for pre-composed special-folder icons (Bilder, Videos, Dokumente, Downloads, …)
   *   whose type symbol is already drawn onto the folder face in the asset.
   */
  variant?: 'native' | 'wrapped' | 'freestanding';
  /** Click handler */
  onClick?: () => void;
  /** Additional inline styles on the outer container */
  style?: React.CSSProperties;
}


/* ---- core/Badge.d.ts ---- */
/**
 * Badge — compact status chip / label for AIVA OS.
 * Use for notification counts, connection status, app state, category tags.
 *
 * @example
 * <Badge variant="active">Connected</Badge>
 * <Badge variant="destructive">Error</Badge>
 * <Badge variant="glass">Tablet Mode</Badge>
 * <Badge variant="success">Online</Badge>
 */
export interface BadgeProps {
  /** Visual variant. Default: 'default' */
  variant?: 'default' | 'secondary' | 'glass' | 'destructive' | 'success' | 'warning' | 'outline' | 'active';
  /** Badge content (usually short text) */
  children?: React.ReactNode;
  /** Additional inline styles */
  style?: React.CSSProperties;
}


/* ---- core/GlassButton.d.ts ---- */
/**
 * GlassButton — The primary interactive button for AIVA OS.
 * Uses liquid glass styling with backdrop-filter blur.
 * Automatically adapts to dark/light mode via the .dark class.
 *
 * @example
 * <GlassButton variant="glass" size="md" onClick={() => {}}>Open</GlassButton>
 * <GlassButton variant="active" icon={<WifiIcon />}>WLAN</GlassButton>
 * <GlassButton variant="destructive" size="sm">Delete</GlassButton>
 * <GlassButton variant="default" size="lg">Launch</GlassButton>
 * <GlassButton variant="ghost" size="icon" icon={<SettingsIcon />} />
 */
export interface GlassButtonProps {
  /** Visual style. Default: 'glass' */
  variant?: 'default' | 'glass' | 'ghost' | 'destructive' | 'active' | 'secondary';
  /** Size preset. Default: 'md' */
  size?: 'sm' | 'md' | 'lg' | 'icon';
  /** Disables interaction and reduces opacity */
  disabled?: boolean;
  /** Click handler */
  onClick?: () => void;
  /** Optional leading icon (any React node, typically an SVG) */
  icon?: React.ReactNode;
  /** Button label / content */
  children?: React.ReactNode;
  /** Additional inline styles */
  style?: React.CSSProperties;
}


/* ---- core/GlassCard.d.ts ---- */
/**
 * GlassCard — frosted glass container. The foundational surface for all
 * AIVA OS panels, dock pills, notification trays, and nested cards.
 * Automatically adapts to dark/light mode via the .dark class.
 *
 * @example
 * <GlassCard variant="panel" style={{ padding: 24 }}>
 *   <h2>Control Center</h2>
 * </GlassCard>
 *
 * <GlassCard variant="card" style={{ padding: 16 }}>
 *   <p>Inner card content</p>
 * </GlassCard>
 */
export interface GlassCardProps {
  /** Surface level. Default: 'panel' */
  variant?: 'panel' | 'dock' | 'card' | 'inner' | 'subtle';
  /** Content to render inside the card */
  children?: React.ReactNode;
  /** Additional inline styles — set width, height, padding, position here */
  style?: React.CSSProperties;
  /** Optional click handler */
  onClick?: () => void;
  /** HTML element to render. Default: 'div' */
  as?: keyof JSX.IntrinsicElements;
}


/* ---- core/GlassToggle.d.ts ---- */
/**
 * GlassToggle — iOS/macOS-style on/off switch for AIVA OS.
 * Used in Control Center (WiFi, Bluetooth, Airplane, Dark Mode) and Settings.
 * Supports controlled and uncontrolled usage.
 *
 * @example
 * // Uncontrolled
 * <GlassToggle defaultChecked label="WLAN" />
 *
 * // Controlled
 * <GlassToggle checked={wifi} onChange={setWifi} label="Bluetooth" />
 *
 * // Toggle only (no label)
 * <GlassToggle checked={darkMode} onChange={setDarkMode} />
 */
export interface GlassToggleProps {
  /** Controlled checked state. If omitted, component manages its own state */
  checked?: boolean;
  /** Initial checked state for uncontrolled usage */
  defaultChecked?: boolean;
  /** Called with the new boolean value when the toggle changes */
  onChange?: (checked: boolean) => void;
  /** Optional text label rendered to the left of the toggle */
  label?: string;
  /** Prevents interaction */
  disabled?: boolean;
  /** Additional inline styles on the outer container */
  style?: React.CSSProperties;
}



/* ============================== FEEDBACK ============================== */

/* ---- feedback/Banner.d.ts ---- */
import React from 'react';

/**
 * Banner — inline status/notification strip (maps to ArkUI ExceptionPrompt).
 * Glass bar with status accent, icon, title/message, optional action and
 * dismiss. Inline (unlike the floating Toast).
 *
 * @example
 * <Banner variant="warning" title="Speicher fast voll" message="Noch 1,2 GB frei." action="Verwalten" onAction={open} />
 * <Banner variant="success" message="Synchronisierung abgeschlossen" onDismiss={hide} />
 */
export interface BannerProps {
  title?: React.ReactNode;
  message?: React.ReactNode;
  variant?: 'info' | 'success' | 'warning' | 'destructive';
  /** Override the default status icon */
  icon?: React.ReactNode;
  /** Action button label */
  action?: string;
  onAction?: () => void;
  /** Shows a dismiss ✕ when provided */
  onDismiss?: () => void;
  style?: React.CSSProperties;
}


/* ---- feedback/ProgressBar.d.ts ---- */
import React from 'react';

/**
 * ProgressBar — glass progress track with a blue fill.
 * Determinate (value 0–100) or indeterminate (sliding pip).
 *
 * @example
 * <ProgressBar value={64} />
 * <ProgressBar indeterminate />
 */
export interface ProgressBarProps {
  /** 0–100; ignored when indeterminate */
  value?: number;
  /** Show an animated indeterminate pip instead of a fixed fill */
  indeterminate?: boolean;
  /** Track height in px */
  height?: number;
  /** Fill color (defaults to the blue active token) */
  color?: string;
  style?: React.CSSProperties;
}


/* ---- feedback/Refresher.d.ts ---- */
import React from 'react';

/**
 * Refresher — pull-to-refresh wrapper (maps to ArkUI SwipeRefresher). Wrap a
 * scrollable region; pulling down past `threshold` reveals a glass spinner and
 * fires onRefresh. End the refresh by calling the passed done() (or resolve a
 * returned Promise).
 *
 * @example
 * <Refresher onRefresh={(done) => { reload().then(done); }} style={{ height: 400 }}>
 *   <List>…</List>
 * </Refresher>
 */
export interface RefresherProps {
  /** Called when the pull crosses the threshold. Receives a done() callback;
   *  alternatively return a Promise that resolves when finished. */
  onRefresh?: (done: () => void) => void | Promise<unknown>;
  /** Pull distance in px required to trigger */
  threshold?: number;
  children?: React.ReactNode;
  style?: React.CSSProperties;
}


/* ---- feedback/Skeleton.d.ts ---- */
import React from 'react';

/**
 * Skeleton — shimmering glass loading placeholder. Size it to the content
 * that's loading; use circle for avatars.
 *
 * @example
 * <Skeleton width={200} height={16} />
 * <Skeleton circle height={48} />
 */
export interface SkeletonProps {
  width?: number | string;
  height?: number | string;
  /** Corner radius (ignored when circle) */
  radius?: number | string;
  circle?: boolean;
  style?: React.CSSProperties;
}

/**
 * SkeletonText — a stack of skeleton lines for paragraph placeholders;
 * the last line is shortened.
 *
 * @example
 * <SkeletonText lines={3} />
 */
export interface SkeletonTextProps {
  lines?: number;
  gap?: number;
  style?: React.CSSProperties;
}


/* ---- feedback/Spinner.d.ts ---- */
import React from 'react';

/**
 * Spinner — iOS-style activity indicator (maps to ion-loading).
 * 12 tapered spokes fading around the circle.
 *
 * @example
 * <Spinner />
 * <Spinner size={20} color="#fff" />
 */
export interface SpinnerProps {
  /** Diameter in px */
  size?: number;
  /** Spoke color (defaults to --glass-text-primary) */
  color?: string;
  style?: React.CSSProperties;
}


/* ---- feedback/Toast.d.ts ---- */
import React from 'react';

/**
 * Toast — transient glass notification (maps to ion-toast). Floating frosted
 * pill with optional icon, status accent dot, and an action button.
 * Auto-dismisses after `duration` ms (set 0 to persist).
 *
 * @example
 * {showToast && (
 *   <Toast message="Datei gespeichert" variant="success" onClose={() => setShow(false)} />
 * )}
 * <Toast message="Verbindung verloren" action="Erneut" onAction={retry} duration={0} />
 */
export interface ToastProps {
  /** Whether the toast is shown */
  open?: boolean;
  message: React.ReactNode;
  /** Leading icon node */
  icon?: React.ReactNode;
  /** Action button label */
  action?: string;
  /** Fired when the action button is pressed */
  onAction?: () => void;
  /** Fired when the auto-dismiss timer elapses */
  onClose?: () => void;
  /** Auto-dismiss delay in ms; 0 disables auto-dismiss */
  duration?: number;
  position?: 'top' | 'bottom' | 'center';
  /** Status accent dot color */
  variant?: 'default' | 'success' | 'warning' | 'destructive';
  style?: React.CSSProperties;
}



/* ============================== INPUTS ============================== */

/* ---- inputs/SearchBar.d.ts ---- */
import React from 'react';

/**
 * SearchBar — iOS-style glass search pill (maps to ion-searchbar).
 * Leading magnifier, clear button when non-empty, Enter submits.
 * Controlled or uncontrolled.
 *
 * @example
 * <SearchBar placeholder="Apps suchen" onSubmit={(q) => run(q)} />
 * <SearchBar value={q} onChange={setQ} />
 */
export interface SearchBarProps {
  value?: string;
  defaultValue?: string;
  onChange?: (value: string) => void;
  /** Fired on Enter with the current value */
  onSubmit?: (value: string) => void;
  placeholder?: string;
  disabled?: boolean;
  style?: React.CSSProperties;
}


/* ---- inputs/TextArea.d.ts ---- */
import React from 'react';

/**
 * TextArea — multiline glass text input with optional label, helper/error,
 * character counter, and auto-grow. Controlled or uncontrolled.
 *
 * @example
 * <TextArea label="Notiz" placeholder="Schreib etwas…" rows={5} />
 * <TextArea value={msg} onChange={setMsg} maxLength={280} showCount autoGrow />
 */
export interface TextAreaProps {
  value?: string;
  defaultValue?: string;
  onChange?: (value: string) => void;
  placeholder?: string;
  label?: string;
  rows?: number;
  /** Hard character limit */
  maxLength?: number;
  /** Show an x/limit counter (requires maxLength) */
  showCount?: boolean;
  /** Grow height to fit content instead of scrolling */
  autoGrow?: boolean;
  error?: string;
  helper?: string;
  disabled?: boolean;
  style?: React.CSSProperties;
  textareaStyle?: React.CSSProperties;
}


/* ---- inputs/TextField.d.ts ---- */
import React from 'react';

/**
 * TextField — glass text input with optional label, leading icon,
 * trailing node, and helper/error text. Controlled or uncontrolled.
 *
 * @example
 * <TextField label="E-Mail" placeholder="name@firma.de" type="email" />
 * <TextField value={pw} onChange={setPw} type="password" error="Zu kurz" />
 */
export interface TextFieldProps {
  value?: string;
  defaultValue?: string;
  onChange?: (value: string) => void;
  placeholder?: string;
  /** Label rendered above the field */
  label?: string;
  /** Native input type */
  type?: string;
  /** Leading icon node */
  icon?: React.ReactNode;
  /** Trailing node (icon button, unit, etc.) */
  trailing?: React.ReactNode;
  /** Error message — turns the border red and shows the text below */
  error?: string;
  /** Helper text below the field (hidden when error is set) */
  helper?: string;
  disabled?: boolean;
  size?: 'sm' | 'md' | 'lg';
  style?: React.CSSProperties;
  /** Styles applied to the inner <input> */
  inputStyle?: React.CSSProperties;
}



/* ============================== NAVIGATION ============================== */

/* ---- navigation/Accordion.d.ts ---- */
import React from 'react';

export interface AccordionItem {
  title: React.ReactNode;
  content: React.ReactNode;
  icon?: React.ReactNode;
}

/**
 * Accordion — collapsible disclosure group in a glass container. Single-open
 * by default; set `multiple` to allow several open at once. Animated height.
 *
 * @example
 * <Accordion defaultOpen={[0]} items={[
 *   { title: 'Allgemein', content: <p>…</p> },
 *   { title: 'Datenschutz', content: <p>…</p> },
 * ]} />
 */
export interface AccordionProps {
  items: AccordionItem[];
  /** Indices open initially */
  defaultOpen?: number[];
  /** Allow multiple sections open simultaneously */
  multiple?: boolean;
  style?: React.CSSProperties;
}


/* ---- navigation/Avatar.d.ts ---- */
import React from 'react';

/**
 * Avatar — circular (or rounded-square) user image with initials fallback
 * on a glass backing, plus an optional presence status dot.
 *
 * @example
 * <Avatar src="/me.jpg" size={48} status="online" />
 * <Avatar initials="LK" size={36} />
 */
export interface AvatarProps {
  /** Image URL; falls back to initials when absent */
  src?: string;
  alt?: string;
  /** Initials shown when there is no image */
  initials?: string;
  /** Diameter in px */
  size?: number;
  /** Presence dot */
  status?: 'online' | 'busy' | 'away' | 'offline';
  /** Rounded-square instead of circle */
  square?: boolean;
  style?: React.CSSProperties;
}


/* ---- navigation/Breadcrumbs.d.ts ---- */
import React from 'react';

export interface Crumb {
  label: string;
  value: string;
}

/**
 * Breadcrumbs — path navigation trail; chevron-separated links with the
 * current page bold and non-interactive.
 *
 * @example
 * <Breadcrumbs items={['Home', 'Dokumente', 'Bericht.pdf']} onNavigate={(v, i) => go(i)} />
 */
export interface BreadcrumbsProps {
  /** Crumbs as strings or {label, value} objects */
  items: (string | Crumb)[];
  /** Fired with (value, index) when a non-last crumb is clicked */
  onNavigate?: (value: string, index: number) => void;
  style?: React.CSSProperties;
}


/* ---- navigation/Chip.d.ts ---- */
import React from 'react';

/**
 * Chip — compact glass token for filters, tags, and recipients. Larger and
 * more tactile than Badge; selectable (blue when selected) and removable.
 *
 * @example
 * <Chip icon={<Tag/>}>Design</Chip>
 * <Chip selected onClick={toggle}>Ungelesen</Chip>
 * <Chip onRemove={() => remove(id)}>anna@firma.de</Chip>
 */
export interface ChipProps {
  children?: React.ReactNode;
  /** Leading icon node */
  icon?: React.ReactNode;
  /** Selected (blue) state */
  selected?: boolean;
  /** Makes the chip tappable */
  onClick?: () => void;
  /** Shows a trailing ✕ that fires this instead of onClick */
  onRemove?: () => void;
  disabled?: boolean;
  style?: React.CSSProperties;
}


/* ---- navigation/ListItem.d.ts ---- */
import React from 'react';

/**
 * ListItem — settings/list row (maps to ion-item). Leading icon/avatar,
 * title + optional subtitle, trailing control or chevron. Interactive
 * when onClick is set.
 *
 * @example
 * <List>
 *   <ListItem leading={<Wifi/>} title="WLAN" subtitle="Verbunden" trailing={<GlassToggle defaultChecked/>} />
 *   <ListItem leading={<Bell/>} title="Mitteilungen" showChevron onClick={open} />
 *   <ListItem title="Abmelden" destructive onClick={signOut} />
 * </List>
 */
export interface ListItemProps {
  title: React.ReactNode;
  subtitle?: React.ReactNode;
  /** Leading icon or avatar */
  leading?: React.ReactNode;
  /** Trailing control (toggle, value text, badge) */
  trailing?: React.ReactNode;
  /** Show a navigation chevron on the right */
  showChevron?: boolean;
  onClick?: () => void;
  /** Render title (and leading) in red */
  destructive?: boolean;
  style?: React.CSSProperties;
}

/**
 * List — grouped glass container that draws hairline dividers between
 * its ListItem children.
 */
export interface ListProps {
  children?: React.ReactNode;
  /** Rounded inset card (true) vs edge-to-edge (false) */
  inset?: boolean;
  style?: React.CSSProperties;
}


/* ---- navigation/Pagination.d.ts ---- */
import React from 'react';

/**
 * Pagination — glass page navigation: prev/next arrows + numbered page pills
 * with ellipsis truncation for large ranges. Controlled or uncontrolled.
 *
 * @example
 * <Pagination count={12} defaultPage={1} onChange={setPage} />
 * <Pagination count={40} page={p} onChange={setP} siblingCount={2} />
 */
export interface PaginationProps {
  /** Total number of pages */
  count: number;
  /** Controlled current page (1-based) */
  page?: number;
  defaultPage?: number;
  onChange?: (page: number) => void;
  /** Pages shown on each side of the current page before truncating */
  siblingCount?: number;
  style?: React.CSSProperties;
}


/* ---- navigation/Sidebar.d.ts ---- */
import React from 'react';

export interface SidebarItem {
  value?: string;
  label: React.ReactNode;
  icon?: React.ReactNode;
  badge?: number;
  onSelect?: () => void;
  /** Render as a non-interactive section label */
  header?: boolean;
}

/**
 * Sidebar — glass navigation rail / drawer (maps to SplitLayout side pane).
 * Vertical list of nav items with active accent highlight, optional header
 * and footer slots. Controlled or uncontrolled.
 *
 * @example
 * <Sidebar value={page} onChange={setPage} header={<Logo/>} items={[
 *   { header: true, label: 'Bibliothek' },
 *   { value: 'all', label: 'Alle Dateien', icon: <Files/> },
 *   { value: 'shared', label: 'Geteilt', icon: <Users/>, badge: 2 },
 * ]} />
 */
export interface SidebarProps {
  items: SidebarItem[];
  value?: string;
  defaultValue?: string;
  onChange?: (value: string) => void;
  header?: React.ReactNode;
  footer?: React.ReactNode;
  width?: number;
  /** 'panel' = frosted glass surface with right border (default); 'plain' = transparent, no border — for embedding inside a window that already has its own surface */
  variant?: 'panel' | 'plain';
  style?: React.CSSProperties;
}

/**
 * SplitView — Sidebar + flexible content two-pane layout.
 *
 * @example
 * <SplitView sidebar={<Sidebar items={nav} />}><Content/></SplitView>
 */
export interface SplitViewProps {
  sidebar: React.ReactNode;
  children?: React.ReactNode;
  style?: React.CSSProperties;
}


/* ---- navigation/SubHeader.d.ts ---- */
import React from 'react';

/**
 * SubHeader — section header row (maps to ArkUI SubHeader). Uppercase title
 * with optional caption and a trailing text action. Place above grouped lists.
 *
 * @example
 * <SubHeader title="Allgemein" action="Alle" onAction={showAll} />
 * <SubHeader title="Geräte" secondary="3 verbunden" />
 */
export interface SubHeaderProps {
  title: React.ReactNode;
  /** Caption under the title */
  secondary?: React.ReactNode;
  /** Trailing text-button label */
  action?: string;
  onAction?: () => void;
  /** Leading icon */
  icon?: React.ReactNode;
  style?: React.CSSProperties;
}


/* ---- navigation/TabBar.d.ts ---- */
import React from 'react';

export interface TabItem {
  value: string;
  label: string;
  icon?: React.ReactNode;
  /** Numeric badge on the icon */
  badge?: number;
}

/**
 * TabBar — bottom tab navigation (maps to ion-tabs). Frosted glass pill of
 * icon+label tabs; active tab tints blue. Controlled or uncontrolled.
 *
 * @example
 * <TabBar value={tab} onChange={setTab} tabs={[
 *   { value: 'home', label: 'Start', icon: <Home/> },
 *   { value: 'msg', label: 'Nachrichten', icon: <Mail/>, badge: 3 },
 *   { value: 'me', label: 'Profil', icon: <User/> },
 * ]} />
 */
export interface TabBarProps {
  tabs: TabItem[];
  value?: string;
  defaultValue?: string;
  onChange?: (value: string) => void;
  /** Floating centered pill (true) vs full-width bar (false) */
  floating?: boolean;
  style?: React.CSSProperties;
}


/* ---- navigation/Toolbar.d.ts ---- */
import React from 'react';

/**
 * Toolbar — top navigation / title bar (maps to ion-toolbar). Frosted glass
 * bar with leading + trailing slots and a leading or centered title.
 *
 * @example
 * <Toolbar title="Einstellungen" centerTitle
 *   leading={<GlassButton variant="ghost" size="icon">‹</GlassButton>}
 *   trailing={<GlassButton variant="ghost" size="icon">⋯</GlassButton>} />
 */
export interface ToolbarProps {
  title?: React.ReactNode;
  subtitle?: React.ReactNode;
  /** Left slot (back button, menu) */
  leading?: React.ReactNode;
  /** Right slot (actions) */
  trailing?: React.ReactNode;
  /** Center the title (iOS nav-bar style) */
  centerTitle?: boolean;
  /** 'panel' frosted bar or 'transparent' (over content) */
  variant?: 'panel' | 'transparent';
  style?: React.CSSProperties;
}


/* ---- navigation/TreeView.d.ts ---- */
import React from 'react';

export interface TreeNode {
  id: string;
  label: React.ReactNode;
  icon?: React.ReactNode;
  children?: TreeNode[];
}

/**
 * TreeView — collapsible hierarchical tree (maps to ArkUI TreeView). Expand
 * chevrons, indentation, icons, selected-row accent. Controlled or uncontrolled.
 *
 * @example
 * <TreeView defaultExpanded={['src']} onSelect={(n) => open(n.id)} nodes={[
 *   { id: 'src', label: 'src', icon: <Folder/>, children: [
 *     { id: 'app', label: 'app.tsx', icon: <File/> },
 *   ]},
 * ]} />
 */
export interface TreeViewProps {
  nodes: TreeNode[];
  /** Controlled selected node id */
  selectedId?: string;
  /** Node ids expanded initially */
  defaultExpanded?: string[];
  onSelect?: (node: TreeNode) => void;
  style?: React.CSSProperties;
}



/* ============================== OVERLAYS ============================== */

/* ---- overlays/ActionSheet.d.ts ---- */
import React from 'react';

export interface SheetAction {
  label: string;
  onPress?: () => void;
  /** Render in red for delete/danger actions */
  destructive?: boolean;
}

/**
 * ActionSheet — bottom glass option list (maps to ion-action-sheet).
 * Grouped action card + separate cancel card, iOS-style. Tapping any
 * action or the backdrop calls onClose.
 *
 * @example
 * <ActionSheet open={open} onClose={close} title="Foto"
 *   actions={[
 *     { label: 'Teilen', onPress: share },
 *     { label: 'Duplizieren', onPress: dup },
 *     { label: 'Löschen', destructive: true, onPress: del },
 *   ]} />
 */
export interface ActionSheetProps {
  open?: boolean;
  onClose?: () => void;
  /** Small bold header text */
  title?: React.ReactNode;
  /** Secondary description under the title */
  message?: React.ReactNode;
  actions: SheetAction[];
  cancelText?: string;
  style?: React.CSSProperties;
}


/* ---- overlays/Alert.d.ts ---- */
import React from 'react';

export interface AlertButton {
  label: string;
  onPress?: () => void;
  /** 'primary' bolds the label, 'destructive' renders it red */
  role?: 'primary' | 'destructive' | 'cancel';
}

/**
 * Alert — iOS-style confirmation dialog (maps to ion-alert). Compact glass
 * card with title, message, and 1–2 buttons (side-by-side when exactly two).
 *
 * @example
 * <Alert open={open} onClose={close} title="Löschen?"
 *   message="Diese Aktion kann nicht rückgängig gemacht werden."
 *   buttons={[
 *     { label: 'Abbrechen', role: 'cancel' },
 *     { label: 'Löschen', role: 'destructive', onPress: del },
 *   ]} />
 */
export interface AlertProps {
  open?: boolean;
  onClose?: () => void;
  title?: React.ReactNode;
  message?: React.ReactNode;
  /** 1–2 buttons; two render side-by-side, more stack vertically */
  buttons?: AlertButton[];
  style?: React.CSSProperties;
}


/* ---- overlays/FAB.d.ts ---- */
import React from 'react';

export interface FABAction {
  /** Icon node shown in the mini-action button */
  icon: React.ReactNode;
  label?: string;
  onPress?: () => void;
}

/**
 * FAB — floating action button (maps to ion-fab). Round glass button; if
 * `actions` are given it rotates to a ✕ and expands a stack of mini-actions.
 * Fixed to a corner by default; use position="static" to inline it.
 *
 * @example
 * <FAB onClick={compose} />
 * <FAB icon={<Plus/>} actions={[
 *   { icon: <Camera/>, label: 'Foto', onPress: photo },
 *   { icon: <Mic/>, label: 'Audio', onPress: audio },
 * ]} />
 */
export interface FABProps {
  /** Main button icon (defaults to a + glyph) */
  icon?: React.ReactNode;
  /** Secondary actions revealed on expand */
  actions?: FABAction[];
  /** Click handler when there are no actions */
  onClick?: () => void;
  position?: 'bottom-end' | 'bottom-start' | 'top-end' | 'top-start' | 'static';
  /** Diameter of the main button in px */
  size?: number;
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
  style?: React.CSSProperties;
}


/* ---- overlays/Menu.d.ts ---- */
import React from 'react';

export interface MenuItem {
  label?: string;
  icon?: React.ReactNode;
  /** Right-aligned shortcut hint, e.g. "⌘C" */
  shortcut?: string;
  onSelect?: () => void;
  destructive?: boolean;
  disabled?: boolean;
  /** Show a checkmark (toggled/selected state) */
  checked?: boolean;
  /** Render as a non-interactive section header */
  header?: boolean;
  /** Render a divider line (ignores other fields) */
  divider?: boolean;
  /** Nested items — renders a flyout submenu on hover, with a chevron hint */
  submenu?: MenuItem[];
}

/**
 * Menu — glass dropdown / context menu (maps to ArkUI Menu / formMenu).
 * Frosted panel of items anchored to a trigger. Click-to-open, or set
 * `contextMenu` for right-click. Dismisses on outside-click / Escape.
 *
 * @example
 * <Menu trigger={<GlassButton variant="glass">Datei ▾</GlassButton>} items={[
 *   { label: 'Öffnen', icon: <Folder/>, shortcut: '⌘O', onSelect: open },
 *   { label: 'Teilen', icon: <Share/>, checked: true },
 *   { divider: true },
 *   { label: 'Löschen', icon: <Trash/>, destructive: true, onSelect: del },
 * ]} />
 */
export interface MenuProps {
  trigger: React.ReactNode;
  items: MenuItem[];
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
  placement?: 'top-start' | 'top-end' | 'bottom-start' | 'bottom-end';
  /** Open on right-click at the cursor instead of click */
  contextMenu?: boolean;
  width?: number;
  style?: React.CSSProperties;
}

/** ContextMenu — right-click `children` to open `items`. */
export interface ContextMenuProps {
  children: React.ReactNode;
  items: MenuItem[];
  width?: number;
  style?: React.CSSProperties;
}


/* ---- overlays/Modal.d.ts ---- */
import React from 'react';

/**
 * Modal — centered glass dialog (maps to ion-modal). Dimmed backdrop,
 * frosted panel, optional title bar / close button / footer slot.
 *
 * @example
 * <Modal open={open} onClose={close} title="Datei umbenennen"
 *        footer={<><GlassButton variant="ghost" onClick={close}>Abbrechen</GlassButton>
 *                  <GlassButton variant="default" onClick={save}>Sichern</GlassButton></>}>
 *   <TextField defaultValue="Bericht.pdf" />
 * </Modal>
 */
export interface ModalProps {
  open?: boolean;
  onClose?: () => void;
  /** Title shown in the header bar */
  title?: React.ReactNode;
  children?: React.ReactNode;
  /** Right-aligned footer actions (buttons) */
  footer?: React.ReactNode;
  /** Max width in px */
  width?: number;
  /** Show the round ✕ close button */
  showClose?: boolean;
  /** Dismiss when the backdrop is clicked */
  dismissOnBackdrop?: boolean;
  style?: React.CSSProperties;
}


/* ---- overlays/Popover.d.ts ---- */
import React from 'react';

/**
 * Popover — anchored floating glass panel (maps to ion-popover). Wraps a
 * trigger; opens a frosted panel positioned relative to it. Dismisses on
 * outside-click / Escape. Pair with PopoverItem for menus.
 *
 * @example
 * <Popover trigger={<GlassButton variant="ghost" size="icon">⋯</GlassButton>}>
 *   <PopoverItem icon={<Share/>}>Teilen</PopoverItem>
 *   <PopoverItem icon={<Trash/>} destructive>Löschen</PopoverItem>
 * </Popover>
 */
export interface PopoverProps {
  /** Element that toggles the popover when clicked */
  trigger: React.ReactNode;
  children?: React.ReactNode;
  /** Controlled open state */
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
  placement?: 'top-start' | 'top-end' | 'bottom-start' | 'bottom-end';
  /** Gap between trigger and panel in px */
  offset?: number;
  /** Fixed panel width in px */
  width?: number;
  style?: React.CSSProperties;
}

/** A tappable row for use inside a Popover menu. */
export interface PopoverItemProps {
  children?: React.ReactNode;
  icon?: React.ReactNode;
  onClick?: () => void;
  /** Render the row red */
  destructive?: boolean;
}


/* ---- overlays/Tooltip.d.ts ---- */
import React from 'react';

/**
 * Tooltip — glass label revealed on hover/focus of the wrapped element.
 *
 * @example
 * <Tooltip label="Neues Fenster" placement="bottom">
 *   <GlassButton variant="glass" size="icon">+</GlassButton>
 * </Tooltip>
 */
export interface TooltipProps {
  /** Text/content shown in the bubble */
  label: React.ReactNode;
  children: React.ReactNode;
  placement?: 'top' | 'bottom' | 'left' | 'right';
  /** Hover delay before showing, ms */
  delay?: number;
  style?: React.CSSProperties;
}



/* ============================== WINDOW ============================== */

/* ---- window/AppWindow.d.ts ---- */
/**
 * AppWindow — the full AIVA OS window scaffold; the single base every
 * app and settings window is built from. Wraps Window with a three-zone
 * body (sidebar · content pane · properties pane), an optional floating
 * action bar, and a responsive layout that lifts the side panes into
 * glass overlays as the window narrows (desktop ≥820 · compact ≥560 ·
 * mobile <560). The sidebar- and properties-toggle chrome buttons are
 * added for you; pass identity chrome via leading/toolbar/trailing.
 *
 * @example
 * <AppWindow theme="dark" leading={appChip} toolbar={navBtns}
 *   sidebar={<Sidebar … />} contentHeader={<Breadcrumbs … />}
 *   properties={propRows} actionBar={<WindowActionBar items={…} />}>
 *   {contentRows}
 * </AppWindow>
 */
export interface AppWindowProps {
  theme?: 'dark' | 'light';
  width?: number;
  height?: number;
  /** Left identity chrome (app-icon chip, app-mode menu) */
  leading?: React.ReactNode;
  /** Centered toolbar cluster (back/forward/search…) */
  toolbar?: React.ReactNode;
  /** Right identity chrome placed before the window controls (app menu) */
  trailing?: React.ReactNode;
  windowControls?: boolean;
  onMinimize?: () => void;
  onMaximize?: () => void;
  onClose?: () => void;
  /** Sidebar element (e.g. a <Sidebar variant="plain" />). Omit for none */
  sidebar?: React.ReactNode;
  sidebarWidth?: number;
  /** Content pane header title (ignored if contentHeader is set) */
  contentTitle?: string;
  /** Custom content pane header node (breadcrumb, sort row…) */
  contentHeader?: React.ReactNode;
  /** Right-aligned actions in the content pane header */
  contentActions?: React.ReactNode;
  contentPadded?: boolean;
  /** Content pane body */
  children?: React.ReactNode;
  /** Properties pane body. Omit for no properties pane */
  properties?: React.ReactNode;
  propertiesTitle?: string;
  propertiesWidth?: number;
  /** Show the properties pane. Default: true */
  showProperties?: boolean;
  /** Floating action bar element (e.g. <WindowActionBar />) */
  actionBar?: React.ReactNode;
  /** Enable the responsive overlay behavior. Default: true */
  responsive?: boolean;
  style?: React.CSSProperties;
  bodyStyle?: React.CSSProperties;
}


/* ---- window/Icon.d.ts ---- */
/**
 * Icon — minimal Lucide-style SVG renderer for AIVA OS window chrome.
 * Pass the raw inner SVG markup (Lucide path data) as `paths`.
 *
 * @example
 * <Icon paths='<path d="M5 12h14"/>' size={14} strokeWidth={2.5} />
 */
export interface IconProps {
  /** Raw inner SVG markup (Lucide path/line/circle elements), viewBox 0 0 24 24 */
  paths: string;
  /** Pixel size (width = height). Default: 16 */
  size?: number;
  /** Stroke width. Default: 2 */
  strokeWidth?: number;
  /** SVG fill. Default: 'none' */
  fill?: string;
  /** Stroke color. Default: 'currentColor' */
  color?: string;
  style?: React.CSSProperties;
}


/* ---- window/Window.d.ts ---- */
/**
 * Window — the shared glass window shell for AIVA OS. Dense window
 * surface + three-zone title chrome (leading / centered toolbar /
 * trailing + minimise·maximise·close) + a flex body region. Every app
 * and settings window composes this; pass the body (Sidebar, WindowPane)
 * as children. Reads the --glass-window-* tokens.
 *
 * @example
 * <Window theme="dark" width={900} leading={appChip} toolbar={navBtns} onClose={close}>
 *   <Sidebar … />
 *   <WindowPane title="Inhalt">{rows}</WindowPane>
 * </Window>
 */
export interface WindowProps {
  /** Scope the window's own theme. Omit to inherit the ambient .dark class */
  theme?: 'dark' | 'light';
  /** Window width in px. Default: 960 */
  width?: number;
  /** Window height in px. Default: 600 */
  height?: number;
  /** Left cluster: app-icon chip, sidebar toggle, etc */
  leading?: React.ReactNode;
  /** Absolutely-centered cluster: back/forward/search toolbar */
  toolbar?: React.ReactNode;
  /** Right cluster, placed before the window controls */
  trailing?: React.ReactNode;
  /** Render the minimise/maximise/close cluster. Default: true */
  windowControls?: boolean;
  onMinimize?: () => void;
  onMaximize?: () => void;
  onClose?: () => void;
  /** Play the spring entrance animation. Default: true */
  animate?: boolean;
  /** Ref attached to the window root element (e.g. for ResizeObserver) */
  rootRef?: React.Ref<HTMLDivElement>;
  className?: string;
  /** Styles for the window shell (override width/height/position) */
  style?: React.CSSProperties;
  /** Styles for the flex body region */
  bodyStyle?: React.CSSProperties;
  children?: React.ReactNode;
}


/* ---- window/WindowActionBar.d.ts ---- */
/**
 * WindowActionBar — floating glass pill of column buttons (icon over
 * label) for the bottom-centre of an AIVA OS window content area.
 *
 * @example
 * <WindowActionBar items={[
 *   { label: 'Neu', icon: newIcon, variant: 'primary', onClick: create },
 *   { divider: true },
 *   { label: 'Löschen', icon: trashIcon, variant: 'danger', onClick: del },
 * ]} />
 */
export interface WindowActionItem {
  label?: string;
  icon?: React.ReactNode;
  /** 'primary' (filled), 'default', 'danger' (red), 'muted' */
  variant?: 'primary' | 'default' | 'danger' | 'muted';
  onClick?: () => void;
  /** Render a vertical divider instead of a button */
  divider?: boolean;
}

export interface WindowActionBarProps {
  items?: WindowActionItem[];
  style?: React.CSSProperties;
}


/* ---- window/WindowButton.d.ts ---- */
/**
 * WindowButton — chrome icon button for AIVA OS window title bars and
 * pane headers. Ghost by default, with `active` (pinned hover) and
 * `danger` (red close-style) treatments. Reads --glass-window-icon /
 * --glass-hover-bg / --glass-text-strong, so it adapts to dark/light.
 *
 * @example
 * <WindowButton title="Suche" onClick={openSearch}>
 *   <Icon paths='<circle cx="11" cy="11" r="8"/><path d="m21 21-4.3-4.3"/>' size={14} />
 * </WindowButton>
 */
export interface WindowButtonProps {
  children?: React.ReactNode;
  onClick?: () => void;
  title?: string;
  /** Pin the hover surface (e.g. a toggled-on panel button). Default: false */
  active?: boolean;
  /** Red destructive treatment (close button). Default: false */
  danger?: boolean;
  /** Square size in px. Default: 30 */
  size?: number;
  /** Border radius in px. Default: 8 */
  radius?: number;
  style?: React.CSSProperties;
}


/* ---- window/WindowControls.d.ts ---- */
/**
 * WindowControls — the minimise / maximise / close cluster for the
 * right edge of an AIVA OS window title bar. Built from WindowButton,
 * so it inherits the dark/light chrome treatment automatically.
 *
 * @example
 * <WindowControls onClose={closeWindow} onMinimize={minimise} />
 */
export interface WindowControlsProps {
  onMinimize?: () => void;
  onMaximize?: () => void;
  onClose?: () => void;
  style?: React.CSSProperties;
}


/* ---- window/WindowPane.d.ts ---- */
/**
 * WindowPane — inner content card inside an AIVA OS window (the content
 * area, the properties panel, etc). Rounded glass card with an optional
 * header (title or custom node + action buttons) and a scrolling body.
 *
 * @example
 * <WindowPane title="Inhalt" headerActions={<WindowButton>…</WindowButton>}>
 *   {rows}
 * </WindowPane>
 */
export interface WindowPaneProps {
  /** Header title text (ignored if `header` is provided) */
  title?: string;
  /** Custom header node, replaces the default title span */
  header?: React.ReactNode;
  /** Right-aligned action buttons in the header */
  headerActions?: React.ReactNode;
  /** Apply default body padding. Default: true */
  padded?: boolean;
  /** Styles for the outer card (set width, flex, etc) */
  style?: React.CSSProperties;
  /** Styles for the scrolling body */
  bodyStyle?: React.CSSProperties;
  children?: React.ReactNode;
}

