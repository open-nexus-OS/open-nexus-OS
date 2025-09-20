//! Toggle buttons (icons, hit tests). Drawing is minimal for now.

use libnexus::themes::{IconVariant, THEME};
use orbclient::{Color, Renderer};
use orbimage::Image;

pub struct Button {
    pub rect: (i32, i32, i32, i32), // x, y, w, h
    pub pressed: bool,
    pub hover: bool,
    pub icon_id: String,
    icon_cached: Option<Image>,
}

impl Button {
    pub fn new(icon_id: String) -> Self {
        Self { rect: (0,0,0,0), pressed: false, hover: false, icon_id, icon_cached: None }
    }

    pub fn set_rect(&mut self, r: (i32,i32,i32,i32)) { self.rect = r; }

    fn load_icon(&mut self, size_px: u32) {
        // (Re)load every frame size change is overkill, but okay for now; you can cache by key.
        self.icon_cached = THEME
            .load_icon_sized(&self.icon_id, IconVariant::Auto, Some((size_px, size_px)))
            .or(Some(Image::default()));
    }

    pub fn hit(&self, x: i32, y: i32) -> bool {
        let (rx, ry, rw, rh) = self.rect;
        x >= rx && x < rx + rw && y >= ry && y < ry + rh
    }

    pub fn draw<R: Renderer>(&mut self, win: &mut R, hover_veil: Color, icon_px: u32) {
        if self.hover || self.pressed {
            let (x,y,w,h) = self.rect;
            win.rect(x - 6, y - 6, (w + 12) as u32, (h + 12) as u32, hover_veil);
        }
        self.load_icon(icon_px);
        if let Some(img) = &self.icon_cached {
            let (x,y,w,h) = self.rect;
            let ix = x + (w - img.width() as i32)/2;
            let iy = y + (h - img.height() as i32)/2;
            img.draw(win, ix, iy);
        }
    }
}
