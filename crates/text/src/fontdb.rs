//! The font database: embedded craft-fonts faces plus (on desktop) the system's fonts.

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use figcraft_geom::BezPath;
use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::raw::{FileRef, TableProvider};
use skrifa::string::StringId;
use skrifa::{GlyphId, MetadataProvider};

/// A font from the optional craft-fonts build input (empty unless built with `CRAFT_FONTS_DIR`).
pub struct CraftFont {
    pub family: &'static str,
    pub style: &'static str,
    pub bytes: &'static [u8],
}

include!(concat!(env!("OUT_DIR"), "/craft_fonts.rs"));

#[derive(Clone)]
pub enum FontBytes {
    Static(&'static [u8]),
    Shared(Arc<Vec<u8>>),
}

impl FontBytes {
    pub fn data(&self) -> &[u8] {
        match self {
            FontBytes::Static(b) => b,
            FontBytes::Shared(v) => v.as_slice(),
        }
    }
}

static NEXT_FACE: AtomicU32 = AtomicU32::new(1);

/// One loaded font face.
pub struct Face {
    pub id: u32,
    pub family: String,
    pub style: String,
    pub weight: u16,
    pub italic: bool,
    pub bytes: FontBytes,
    pub index: u32,
    pub upem: f64,
    /// Ascent and descent in font units, both positive.
    pub ascent: f64,
    pub descent: f64,
    pub line_gap: f64,
    pub cap_height: f64,
    pub x_height: f64,
    pub underline_pos: f64,
    pub underline_thickness: f64,
    pub strike_pos: f64,
    pub shaper: harfrust::ShaperData,
    charmap: HashMap<u32, u32>,
    outlines: Mutex<HashMap<u32, Arc<BezPath>>>,
    advances: Mutex<HashMap<u32, f64>>,
}

impl std::fmt::Debug for Face {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Face({} {} #{})", self.family, self.style, self.id)
    }
}

impl Face {
    pub fn from_bytes(bytes: FontBytes, index: u32, family_override: Option<&str>, style_override: Option<&str>) -> Option<Face> {
        let data = bytes.data();
        let f = skrifa::FontRef::from_index(data, index).ok()?;
        let (family, style) = names(&f);
        let family = family_override.map(str::to_string).or(family)?;
        let style = style_override.map(str::to_string).or(style).unwrap_or_else(|| "Regular".into());
        let m = f.metrics(Size::unscaled(), LocationRef::default());
        let attrs = f.attributes();
        let hb = harfrust::FontRef::from_index(data, index).ok()?;
        let shaper = harfrust::ShaperData::new(&hb);
        let upem = f64::from(m.units_per_em.max(1));
        let ascent = f64::from(m.ascent);
        let descent = -f64::from(m.descent);
        let charmap: HashMap<u32, u32> = f.charmap().mappings().map(|(c, g)| (c, g.to_u32())).collect();
        let under = m.underline.map(|d| (f64::from(d.offset), f64::from(d.thickness))).unwrap_or((-upem * 0.1, upem * 0.06));
        let strike = m.strikeout.map(|d| f64::from(d.offset)).unwrap_or(ascent * 0.3);
        let weight = {
            let w = attrs.weight.value();
            if w.is_finite() && w > 0.0 { w.round() as u16 } else { figcraft_doc::style_weight(&style) }
        };
        Some(Face {
            id: NEXT_FACE.fetch_add(1, Ordering::Relaxed),
            italic: !matches!(attrs.style, skrifa::attribute::Style::Normal) || style.to_ascii_lowercase().contains("italic"),
            weight,
            family,
            style,
            index,
            upem,
            ascent,
            descent,
            line_gap: f64::from(m.leading),
            cap_height: m.cap_height.map(f64::from).filter(|v| *v > 0.0).unwrap_or(ascent * 0.72),
            x_height: m.x_height.map(f64::from).filter(|v| *v > 0.0).unwrap_or(ascent * 0.5),
            underline_pos: under.0,
            underline_thickness: under.1.max(upem * 0.02),
            strike_pos: strike,
            shaper,
            charmap,
            bytes,
            outlines: Mutex::new(HashMap::new()),
            advances: Mutex::new(HashMap::new()),
        })
    }

    pub fn has_char(&self, c: char) -> bool {
        self.charmap.get(&(c as u32)).is_some_and(|g| *g != 0)
    }

    pub fn glyph_for(&self, c: char) -> u32 {
        self.charmap.get(&(c as u32)).copied().unwrap_or(0)
    }

    pub fn hb(&self) -> Option<harfrust::FontRef<'_>> {
        harfrust::FontRef::from_index(self.bytes.data(), self.index).ok()
    }

    /// Glyph outline in font units, y down.
    pub fn outline(&self, gid: u32) -> Arc<BezPath> {
        {
            let cache = self.outlines.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(p) = cache.get(&gid) {
                return p.clone();
            }
        }
        let mut pen = FlipPen(BezPath::new());
        if let Ok(f) = skrifa::FontRef::from_index(self.bytes.data(), self.index)
            && let Some(g) = f.outline_glyphs().get(GlyphId::new(gid))
        {
            let _ = g.draw(DrawSettings::unhinted(Size::unscaled(), LocationRef::default()), &mut pen);
        }
        let p = Arc::new(pen.0);
        let mut cache = self.outlines.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if cache.len() > 20_000 {
            cache.clear();
        }
        cache.insert(gid, p.clone());
        p
    }

    /// Horizontal advance in font units.
    pub fn advance(&self, gid: u32) -> f64 {
        {
            let cache = self.advances.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(a) = cache.get(&gid) {
                return *a;
            }
        }
        let adv = skrifa::FontRef::from_index(self.bytes.data(), self.index)
            .ok()
            .and_then(|f| f.glyph_metrics(Size::unscaled(), LocationRef::default()).advance_width(GlyphId::new(gid)))
            .map(f64::from)
            .unwrap_or(self.upem * 0.5);
        self.advances.lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert(gid, adv);
        adv
    }
}

struct FlipPen(BezPath);

impl OutlinePen for FlipPen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.move_to((f64::from(x), -f64::from(y)));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.line_to((f64::from(x), -f64::from(y)));
    }
    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.0.quad_to((f64::from(cx0), -f64::from(cy0)), (f64::from(x), -f64::from(y)));
    }
    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.0.curve_to((f64::from(cx0), -f64::from(cy0)), (f64::from(cx1), -f64::from(cy1)), (f64::from(x), -f64::from(y)));
    }
    fn close(&mut self) {
        self.0.close_path();
    }
}

fn name_string(f: &skrifa::FontRef, id: StringId) -> Option<String> {
    let s: String = f.localized_strings(id).english_or_first()?.chars().collect();
    let s = s.trim().to_string();
    (!s.is_empty()).then_some(s)
}

/// (family, style) preferring the typographic names.
fn names(f: &skrifa::FontRef) -> (Option<String>, Option<String>) {
    let family = name_string(f, StringId::TYPOGRAPHIC_FAMILY_NAME).or_else(|| name_string(f, StringId::FAMILY_NAME));
    let style = name_string(f, StringId::TYPOGRAPHIC_SUBFAMILY_NAME).or_else(|| name_string(f, StringId::SUBFAMILY_NAME));
    (family, style)
}

/// A system font file entry not yet loaded.
#[derive(Clone, Debug)]
struct Pending {
    path: std::path::PathBuf,
    index: u32,
    style: String,
}

#[derive(Default)]
pub struct FontDb {
    faces: Vec<Arc<Face>>,
    /// Lower-case family → (display family, face indices).
    families: BTreeMap<String, (String, Vec<usize>)>,
    /// System faces known by name but not loaded (lower-case family → entries).
    pending: BTreeMap<String, (String, Vec<Pending>)>,
    system_scanned: bool,
}

static DB: OnceLock<RwLock<FontDb>> = OnceLock::new();

/// The process-wide font database (embedded fonts loaded on first use).
pub fn db() -> &'static RwLock<FontDb> {
    DB.get_or_init(|| {
        let mut db = FontDb::default();
        for f in CRAFT_FONTS {
            if let Some(face) = Face::from_bytes(FontBytes::Static(f.bytes), 0, Some(f.family), Some(f.style)) {
                db.add(face);
            }
        }
        RwLock::new(db)
    })
}

impl FontDb {
    pub fn add(&mut self, face: Face) -> Arc<Face> {
        let key = face.family.to_lowercase();
        let display = face.family.clone();
        let face = Arc::new(face);
        self.faces.push(face.clone());
        let idx = self.faces.len() - 1;
        self.families.entry(key).or_insert_with(|| (display, Vec::new())).1.push(idx);
        face
    }

    /// Add font file bytes (all faces of a collection). Returns the faces added.
    pub fn add_bytes(&mut self, bytes: Vec<u8>) -> Vec<Arc<Face>> {
        let shared = Arc::new(bytes);
        let count = match FileRef::new(shared.as_slice()) {
            Ok(FileRef::Collection(c)) => c.len(),
            Ok(FileRef::Font(_)) => 1,
            Err(_) => 0,
        };
        (0..count).filter_map(|i| Face::from_bytes(FontBytes::Shared(shared.clone()), i, None, None)).map(|f| self.add(f)).collect()
    }

    /// Family display names, sorted (loaded and known system families).
    pub fn families(&self) -> Vec<String> {
        let mut v: Vec<String> = self.families.values().map(|(d, _)| d.clone()).collect();
        for (k, (d, _)) in &self.pending {
            if !self.families.contains_key(k) {
                v.push(d.clone());
            }
        }
        v.sort_by_key(|a| a.to_lowercase());
        v.dedup();
        v
    }

    /// Style names of a family, by weight then italic.
    pub fn styles(&mut self, family: &str) -> Vec<String> {
        self.load_family(family);
        let Some((_, idx)) = self.families.get(&family.to_lowercase()) else { return vec![] };
        let mut v: Vec<(u16, bool, String)> = idx.iter().filter_map(|i| self.faces.get(*i)).map(|f| (f.weight, f.italic, f.style.clone())).collect();
        v.sort();
        v.dedup_by(|a, b| a.2 == b.2);
        v.into_iter().map(|(_, _, s)| s).collect()
    }

    pub fn has_family(&self, family: &str) -> bool {
        let k = family.to_lowercase();
        self.families.contains_key(&k) || self.pending.contains_key(&k)
    }

    fn load_family(&mut self, family: &str) {
        let key = family.to_lowercase();
        let Some((_, entries)) = self.pending.remove(&key) else { return };
        let mut by_path: BTreeMap<std::path::PathBuf, Arc<Vec<u8>>> = BTreeMap::new();
        for e in entries {
            let bytes = match by_path.get(&e.path) {
                Some(b) => b.clone(),
                None => match std::fs::read(&e.path) {
                    Ok(b) => {
                        let b = Arc::new(b);
                        by_path.insert(e.path.clone(), b.clone());
                        b
                    }
                    Err(_) => continue,
                },
            };
            if let Some(face) = Face::from_bytes(FontBytes::Shared(bytes), e.index, None, Some(&e.style)) {
                self.add(face);
            }
        }
    }

    /// The best face for `family` + `style` (exact style name, else nearest weight/italic).
    pub fn find(&mut self, family: &str, style: &str) -> Option<Arc<Face>> {
        self.load_family(family);
        let (_, idx) = self.families.get(&family.to_lowercase())?;
        let faces: Vec<&Arc<Face>> = idx.iter().filter_map(|i| self.faces.get(*i)).collect();
        let norm = |s: &str| s.to_lowercase().replace([' ', '-'], "");
        if let Some(f) = faces.iter().find(|f| norm(&f.style) == norm(style)) {
            return Some((*f).clone());
        }
        let want_w = i32::from(figcraft_doc::style_weight(style));
        let want_i = style.to_lowercase().contains("italic");
        faces.into_iter().min_by_key(|f| (i32::from(f.weight) - want_w).abs() + if f.italic == want_i { 0 } else { 1000 }).cloned()
    }

    /// The default face (Inter Regular if embedded, else any system sans).
    pub fn default_face(&mut self) -> Option<Arc<Face>> {
        for fam in ["Inter", "Helvetica Neue", "Helvetica", "Arial", "Segoe UI", "Roboto", "DejaVu Sans", "Liberation Sans", "Noto Sans"] {
            if let Some(f) = self.find(fam, "Regular") {
                return Some(f);
            }
        }
        self.faces.first().cloned()
    }

    /// Faces to try for characters the primary face lacks.
    pub fn fallbacks(&mut self) -> Vec<Arc<Face>> {
        let mut out = Vec::new();
        for fam in [
            "Inter",
            "Helvetica Neue",
            "Arial",
            "Segoe UI",
            "DejaVu Sans",
            "Noto Sans",
            "BIZ UDPGothic",
            "Hiragino Sans",
            "PingFang SC",
            "Noto Sans CJK SC",
            "Microsoft YaHei",
            "Apple Color Emoji",
            "Noto Sans Arabic",
            "Apple Symbols",
            "Segoe UI Symbol",
        ] {
            if let Some(f) = self.find(fam, "Regular") {
                out.push(f);
            }
        }
        out
    }

    pub fn face_count(&self) -> usize {
        self.faces.len()
    }

    /// Index the system font directories (desktop only): family names are read now, faces loaded
    /// when first used. Safe to call more than once.
    pub fn scan_system(&mut self) {
        if self.system_scanned {
            return;
        }
        self.system_scanned = true;
        for dir in system_font_dirs() {
            scan_dir(&dir, 0, &mut self.pending);
        }
        // Families already embedded win.
        let embedded: Vec<String> = self.families.keys().cloned().collect();
        for k in embedded {
            self.pending.remove(&k);
        }
    }
}

fn system_font_dirs() -> Vec<std::path::PathBuf> {
    #[allow(unused_mut)]
    let mut v: Vec<std::path::PathBuf> = Vec::new();
    #[cfg(target_os = "macos")]
    {
        v.push("/System/Library/Fonts".into());
        v.push("/Library/Fonts".into());
        if let Some(h) = std::env::var_os("HOME") {
            v.push(std::path::PathBuf::from(h).join("Library/Fonts"));
        }
    }
    #[cfg(target_os = "windows")]
    {
        let windir = std::env::var_os("WINDIR").map(std::path::PathBuf::from).unwrap_or_else(|| "C:\\Windows".into());
        v.push(windir.join("Fonts"));
        if let Some(l) = std::env::var_os("LOCALAPPDATA") {
            v.push(std::path::PathBuf::from(l).join("Microsoft\\Windows\\Fonts"));
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        v.push("/usr/share/fonts".into());
        v.push("/usr/local/share/fonts".into());
        if let Some(h) = std::env::var_os("HOME") {
            v.push(std::path::PathBuf::from(&h).join(".fonts"));
            v.push(std::path::PathBuf::from(h).join(".local/share/fonts"));
        }
    }
    v
}

fn scan_dir(dir: &std::path::Path, depth: usize, out: &mut BTreeMap<String, (String, Vec<Pending>)>) {
    if depth > 6 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for entry in rd.flatten() {
        let path = entry.path();
        if path.is_dir() {
            scan_dir(&path, depth + 1, out);
            continue;
        }
        let ext = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).unwrap_or_default();
        if !matches!(ext.as_str(), "ttf" | "otf" | "ttc" | "otc") {
            continue;
        }
        // Skip very large files (colour emoji); they are not outline fonts we can draw anyway.
        if std::fs::metadata(&path).map(|m| m.len() > 64 * 1024 * 1024).unwrap_or(true) {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else { continue };
        let count = match FileRef::new(bytes.as_slice()) {
            Ok(FileRef::Collection(c)) => c.len(),
            Ok(FileRef::Font(_)) => 1,
            Err(_) => 0,
        };
        for i in 0..count.min(64) {
            let Ok(f) = skrifa::FontRef::from_index(bytes.as_slice(), i) else { continue };
            // Only outline fonts we can draw.
            if f.glyf().is_err() && f.cff().is_err() && f.cff2().is_err() {
                continue;
            }
            let (Some(family), style) = names(&f) else { continue };
            if family.starts_with('.') {
                continue;
            }
            let style = style.unwrap_or_else(|| "Regular".into());
            out.entry(family.to_lowercase()).or_insert_with(|| (family.clone(), Vec::new())).1.push(Pending { path: path.clone(), index: i, style });
        }
    }
}

/// Convenience: find a face with the global db.
pub fn find_face(family: &str, style: &str) -> Option<Arc<Face>> {
    let mut db = db().write().unwrap_or_else(std::sync::PoisonError::into_inner);
    db.find(family, style).or_else(|| db.default_face())
}

/// All family names known to the global db.
pub fn family_names() -> Vec<String> {
    db().read().unwrap_or_else(std::sync::PoisonError::into_inner).families()
}

/// Style names of a family in the global db.
pub fn family_styles(family: &str) -> Vec<String> {
    db().write().unwrap_or_else(std::sync::PoisonError::into_inner).styles(family)
}

/// Scan system fonts into the global db (desktop; a no-op on the web). The directory walk runs
/// without holding the database lock; results are merged in one short write.
pub fn scan_system_fonts() {
    #[cfg(not(target_arch = "wasm32"))]
    {
        if db().read().unwrap_or_else(std::sync::PoisonError::into_inner).system_scanned {
            return;
        }
        let mut found: BTreeMap<String, (String, Vec<Pending>)> = BTreeMap::new();
        for dir in system_font_dirs() {
            scan_dir(&dir, 0, &mut found);
        }
        let mut w = db().write().unwrap_or_else(std::sync::PoisonError::into_inner);
        if w.system_scanned {
            return;
        }
        w.system_scanned = true;
        for (k, v) in found {
            if !w.families.contains_key(&k) {
                w.pending.entry(k).or_insert(v);
            }
        }
        drop(w);
        crate::clear_layout_cache();
    }
}
