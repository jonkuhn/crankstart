use {
    crate::{
        geometry::{ScreenPoint, ScreenRect, ScreenSize, ScreenVector},
        log_to_console, pd_func_caller, pd_func_caller_log,
        system::System,
    },
    alloc::{format, rc::Rc},
    anyhow::{anyhow, ensure, Error},
    core::{cell::RefCell, ops::RangeInclusive, ptr, slice},
    crankstart_sys::{ctypes::c_int, size_t, LCDBitmapTable, LCDPattern},
    cstr_core::{CStr, CString},
    euclid::default::{Point2D, Vector2D},
    hashbrown::HashMap,
};

pub use crankstart_sys::{
    LCDBitmapDrawMode, LCDBitmapFlip, LCDLineCapStyle, LCDRect, LCDSolidColor, PDRect,
    PDStringEncoding, LCD_COLUMNS, LCD_ROWS, LCD_ROWSIZE,
};

pub fn rect_make(x: f32, y: f32, width: f32, height: f32) -> PDRect {
    PDRect {
        x,
        y,
        width,
        height,
    }
}

pub enum LCDColor {
    Solid(LCDSolidColor),
    Pattern(LCDPattern),
}

impl From<LCDColor> for usize {
    fn from(color: LCDColor) -> Self {
        match color {
            LCDColor::Solid(solid_color) => solid_color as usize,
            LCDColor::Pattern(pattern) => {
                let pattern_ptr = &pattern as *const u8;
                pattern_ptr as usize
            }
        }
    }
}

#[derive(Debug)]
pub struct BitmapData {
    pub width: c_int,
    pub height: c_int,
    pub rowbytes: c_int,
    pub hasmask: bool,
}

#[derive(Debug)]
pub struct BitmapInner {
    pub(crate) raw_bitmap: *mut crankstart_sys::LCDBitmap,
}

impl BitmapInner {
    pub fn get_data(&self) -> Result<BitmapData, Error> {
        let mut width = 0;
        let mut height = 0;
        let mut rowbytes = 0;
        let mut hasmask = 0;
        pd_func_caller!(
            (*Graphics::get_ptr()).getBitmapData,
            self.raw_bitmap,
            &mut width,
            &mut height,
            &mut rowbytes,
            &mut hasmask,
            ptr::null_mut(),
        )?;
        Ok(BitmapData {
            width,
            height,
            rowbytes,
            hasmask: hasmask != 0,
        })
    }

    pub fn draw(
        &self,
        target: OptionalBitmap,
        stencil: OptionalBitmap,
        location: ScreenPoint,
        mode: LCDBitmapDrawMode,
        flip: LCDBitmapFlip,
        clip: LCDRect,
    ) -> Result<(), Error> {
        pd_func_caller!(
            (*Graphics::get_ptr()).drawBitmap,
            self.raw_bitmap,
            raw_bitmap(target),
            raw_bitmap(stencil),
            location.x,
            location.y,
            mode.into(),
            flip.into(),
            clip,
        )?;
        Ok(())
    }

    pub fn draw_scaled(
        &self,
        target: OptionalBitmap,
        stencil: OptionalBitmap,
        location: ScreenPoint,
        scale: Vector2D<f32>,
        mode: LCDBitmapDrawMode,
        clip: LCDRect,
    ) -> Result<(), Error> {
        pd_func_caller!(
            (*Graphics::get_ptr()).drawScaledBitmap,
            self.raw_bitmap,
            raw_bitmap(target),
            raw_bitmap(stencil),
            location.x,
            location.y,
            scale.x,
            scale.y,
            mode.into(),
            clip,
        )
    }

    pub fn tile(
        &self,
        target: OptionalBitmap,
        stencil: OptionalBitmap,
        location: ScreenPoint,
        size: ScreenSize,
        mode: LCDBitmapDrawMode,
        flip: LCDBitmapFlip,
        clip: LCDRect,
    ) -> Result<(), Error> {
        pd_func_caller!(
            (*Graphics::get_ptr()).tileBitmap,
            self.raw_bitmap,
            raw_bitmap(target),
            raw_bitmap(stencil),
            location.x,
            location.y,
            size.width,
            size.height,
            mode.into(),
            flip.into(),
            clip,
        )?;
        Ok(())
    }

    pub fn clear(&self, color: LCDColor) -> Result<(), Error> {
        pd_func_caller!(
            (*Graphics::get_ptr()).clearBitmap,
            self.raw_bitmap,
            color.into()
        )
    }

    pub fn duplicate(&self) -> Result<Self, Error> {
        let raw_bitmap = pd_func_caller!((*Graphics::get_ptr()).copyBitmap, self.raw_bitmap)?;

        Ok(Self { raw_bitmap })
    }

    pub fn transform(&self, rotation: f32, scale: Vector2D<f32>) -> Result<Self, Error> {
        let raw_bitmap = pd_func_caller!(
            (*Graphics::get_ptr()).transformedBitmap,
            self.raw_bitmap,
            rotation,
            scale.x,
            scale.y,
            core::ptr::null_mut(),
        )?;
        Ok(Self { raw_bitmap })
    }

    pub fn into_color(&self, bitmap: Bitmap, top_left: Point2D<i32>) -> Result<LCDColor, Error> {
        let mut pattern = LCDPattern::default();
        let pattern_ptr = pattern.as_mut_ptr();
        let mut pattern_val = pattern_ptr as usize;
        let graphics = Graphics::get();
        pd_func_caller!(
            (*graphics.0).setColorToPattern,
            &mut pattern_val,
            self.raw_bitmap,
            top_left.x,
            top_left.y
        )?;
        Ok(LCDColor::Pattern(pattern))
    }

    pub fn load(&self, path: &str) -> Result<(), Error> {
        let c_path = CString::new(path).map_err(Error::msg)?;
        let mut out_err: *const crankstart_sys::ctypes::c_char = ptr::null_mut();
        let graphics = Graphics::get();
        pd_func_caller!(
            (*graphics.0).loadIntoBitmap,
            c_path.as_ptr(),
            self.raw_bitmap,
            &mut out_err
        )?;
        if out_err != ptr::null_mut() {
            let err_msg = unsafe { CStr::from_ptr(out_err).to_string_lossy().into_owned() };
            Err(anyhow!(err_msg))
        } else {
            Ok(())
        }
    }

    pub fn check_mask_collision(
        &self,
        my_location: ScreenPoint,
        my_flip: LCDBitmapFlip,
        other: Bitmap,
        other_location: ScreenPoint,
        other_flip: LCDBitmapFlip,
        rect: ScreenRect,
    ) -> Result<bool, Error> {
        let graphics = Graphics::get();
        let other_raw = other.inner.borrow().raw_bitmap;
        let lcd_rect: LCDRect = rect.to_untyped().into();
        let pixels_covered = pd_func_caller!(
            (*graphics.0).checkMaskCollision,
            self.raw_bitmap,
            my_location.x,
            my_location.y,
            my_flip,
            other_raw,
            other_location.x,
            other_location.y,
            other_flip,
            lcd_rect,
        )?;
        Ok(pixels_covered != 0)
    }
}

impl Drop for BitmapInner {
    fn drop(&mut self) {
        pd_func_caller_log!((*Graphics::get_ptr()).freeBitmap, self.raw_bitmap);
    }
}

pub type BitmapInnerPtr = Rc<RefCell<BitmapInner>>;

#[derive(Clone, Debug)]
pub struct Bitmap {
    pub(crate) inner: BitmapInnerPtr,
}

impl Bitmap {
    fn new(raw_bitmap: *mut crankstart_sys::LCDBitmap) -> Self {
        Bitmap {
            inner: Rc::new(RefCell::new(BitmapInner { raw_bitmap })),
        }
    }

    pub fn get_data(&self) -> Result<BitmapData, Error> {
        self.inner.borrow().get_data()
    }

    pub fn draw(
        &self,
        target: OptionalBitmap,
        stencil: OptionalBitmap,
        location: ScreenPoint,
        mode: LCDBitmapDrawMode,
        flip: LCDBitmapFlip,
        clip: LCDRect,
    ) -> Result<(), Error> {
        self.inner
            .borrow()
            .draw(target, stencil, location, mode, flip, clip)
    }

    pub fn draw_scaled(
        &self,
        target: OptionalBitmap,
        stencil: OptionalBitmap,
        location: ScreenPoint,
        scale: Vector2D<f32>,
        mode: LCDBitmapDrawMode,
        flip: LCDBitmapFlip,
        clip: LCDRect,
    ) -> Result<(), Error> {
        self.inner
            .borrow()
            .draw_scaled(target, stencil, location, scale, mode, clip)
    }

    pub fn tile(
        &self,
        target: OptionalBitmap,
        stencil: OptionalBitmap,
        location: ScreenPoint,
        size: ScreenSize,
        mode: LCDBitmapDrawMode,
        flip: LCDBitmapFlip,
        clip: LCDRect,
    ) -> Result<(), Error> {
        self.inner
            .borrow()
            .tile(target, stencil, location, size, mode, flip, clip)
    }

    pub fn clear(&self, color: LCDColor) -> Result<(), Error> {
        self.inner.borrow().clear(color)
    }

    pub fn transform(&self, rotation: f32, scale: Vector2D<f32>) -> Result<Bitmap, Error> {
        let inner = self.inner.borrow().transform(rotation, scale)?;
        Ok(Self {
            inner: Rc::new(RefCell::new(inner)),
        })
    }

    pub fn into_color(&self, bitmap: Bitmap, top_left: Point2D<i32>) -> Result<LCDColor, Error> {
        self.inner.borrow().into_color(bitmap, top_left)
    }

    pub fn load(&self, path: &str) -> Result<(), Error> {
        self.inner.borrow().load(path)
    }

    pub fn check_mask_collision(
        &self,
        my_location: ScreenPoint,
        my_flip: LCDBitmapFlip,
        other: Bitmap,
        other_location: ScreenPoint,
        other_flip: LCDBitmapFlip,
        rect: ScreenRect,
    ) -> Result<bool, Error> {
        self.inner.borrow().check_mask_collision(
            my_location,
            my_flip,
            other,
            other_location,
            other_flip,
            rect,
        )
    }
}

type OptionalBitmap<'a> = Option<&'a mut Bitmap>;

fn raw_bitmap(bitmap: OptionalBitmap<'_>) -> *mut crankstart_sys::LCDBitmap {
    if let Some(bitmap) = bitmap {
        bitmap.inner.borrow().raw_bitmap
    } else {
        ptr::null_mut() as *mut crankstart_sys::LCDBitmap
    }
}

pub struct Font(*mut crankstart_sys::LCDFont);

impl Font {
    pub fn new(font: *mut crankstart_sys::LCDFont) -> Result<Self, Error> {
        anyhow::ensure!(font != ptr::null_mut(), "Null pointer passed to Font::new");
        Ok(Self(font))
    }

    pub fn get_glyph(&self, c: u16) -> Result<(Bitmap, u32), Error> {
        let graphics = Graphics::get();
        let mut advance = 0;
        let raw_bitmap = pd_func_caller!((*graphics.0).getFontGlyph, self.0, c, &mut advance)?;
        Ok((Bitmap::new(raw_bitmap), advance))
    }

    pub fn get_kerning(&self, c1: u16, c2: u16) -> Result<i32, Error> {
        let graphics = Graphics::get();
        pd_func_caller!((*graphics.0).getFontKerning, self.0, c1, c2)
    }
}

impl Drop for Font {
    fn drop(&mut self) {
        log_to_console!("Leaking a font");
    }
}

#[derive(Debug)]
struct BitmapTableInner {
    raw_bitmap_table: *mut LCDBitmapTable,
    bitmaps: HashMap<usize, Bitmap>,
}

impl BitmapTableInner {
    fn get_bitmap(&mut self, index: usize) -> Result<Bitmap, Error> {
        if let Some(bitmap) = self.bitmaps.get(&index) {
            Ok(bitmap.clone())
        } else {
            let raw_bitmap = pd_func_caller!(
                (*Graphics::get_ptr()).getTableBitmap,
                self.raw_bitmap_table,
                index as c_int
            )?;
            ensure!(
                raw_bitmap != ptr::null_mut(),
                "Failed to load bitmap {} from table {:?}",
                index,
                self.raw_bitmap_table
            );
            let bitmap = Bitmap::new(raw_bitmap);
            self.bitmaps.insert(index, bitmap.clone());
            Ok(bitmap)
        }
    }

    fn load(&mut self, path: &str) -> Result<(), Error> {
        let c_path = CString::new(path).map_err(Error::msg)?;
        let mut out_err: *const crankstart_sys::ctypes::c_char = ptr::null_mut();
        let graphics = Graphics::get();
        pd_func_caller!(
            (*graphics.0).loadIntoBitmapTable,
            c_path.as_ptr(),
            self.raw_bitmap_table,
            &mut out_err
        )?;
        if out_err != ptr::null_mut() {
            let err_msg = unsafe { CStr::from_ptr(out_err).to_string_lossy().into_owned() };
            Err(anyhow!(err_msg))
        } else {
            Ok(())
        }
    }
}

impl Drop for BitmapTableInner {
    fn drop(&mut self) {
        pd_func_caller_log!(
            (*Graphics::get_ptr()).freeBitmapTable,
            self.raw_bitmap_table
        );
    }
}

type BitmapTableInnerPtr = Rc<RefCell<BitmapTableInner>>;

#[derive(Clone, Debug)]
pub struct BitmapTable {
    inner: BitmapTableInnerPtr,
}

impl BitmapTable {
    pub fn new(raw_bitmap_table: *mut LCDBitmapTable) -> Self {
        Self {
            inner: Rc::new(RefCell::new(BitmapTableInner {
                raw_bitmap_table,
                bitmaps: HashMap::new(),
            })),
        }
    }

    pub fn load(&self, path: &str) -> Result<(), Error> {
        self.inner.borrow_mut().load(path)
    }

    pub fn get_bitmap(&self, index: usize) -> Result<Bitmap, Error> {
        self.inner.borrow_mut().get_bitmap(index)
    }
}

static mut GRAPHICS: Graphics = Graphics(ptr::null_mut());

#[derive(Clone, Debug)]
pub struct Graphics(*mut crankstart_sys::playdate_graphics);

impl Graphics {
    pub(crate) fn new(graphics: *mut crankstart_sys::playdate_graphics) {
        unsafe {
            GRAPHICS = Self(graphics);
        }
    }

    pub fn get() -> Self {
        unsafe { GRAPHICS.clone() }
    }

    pub fn get_ptr() -> *mut crankstart_sys::playdate_graphics {
        Self::get().0
    }

    pub fn get_frame(&self) -> Result<&'static mut [u8], Error> {
        let ptr = pd_func_caller!((*self.0).getFrame)?;
        anyhow::ensure!(
            ptr != ptr::null_mut(),
            "Null pointer returned from getFrame"
        );
        let frame = unsafe { slice::from_raw_parts_mut(ptr, (LCD_ROWSIZE * LCD_ROWS) as usize) };
        Ok(frame)
    }

    pub fn get_display_frame(&self) -> Result<&'static mut [u8], Error> {
        let ptr = pd_func_caller!((*self.0).getDisplayFrame)?;
        anyhow::ensure!(
            ptr != ptr::null_mut(),
            "Null pointer returned from getDisplayFrame"
        );
        let frame = unsafe { slice::from_raw_parts_mut(ptr, (LCD_ROWSIZE * LCD_ROWS) as usize) };
        Ok(frame)
    }

    pub fn get_debug_image(&self) -> Result<Bitmap, Error> {
        let raw_bitmap = pd_func_caller!((*self.0).getDebugImage)?;
        anyhow::ensure!(
            raw_bitmap != ptr::null_mut(),
            "Null pointer returned from getDebugImage"
        );
        Ok(Bitmap::new(raw_bitmap))
    }

    pub fn get_framebuffer_bitmap(&self) -> Result<Bitmap, Error> {
        let raw_bitmap = pd_func_caller!((*self.0).getFrameBufferBitmap)?;
        anyhow::ensure!(
            raw_bitmap != ptr::null_mut(),
            "Null pointer returned from getFrameBufferBitmap"
        );
        Ok(Bitmap::new(raw_bitmap))
    }

    pub fn set_background_color(&self, color: LCDSolidColor) -> Result<(), Error> {
        pd_func_caller!((*self.0).setBackgroundColor, color.into())
    }

    pub fn mark_updated_rows(&self, range: RangeInclusive<i32>) -> Result<(), Error> {
        let (start, end) = range.into_inner();
        pd_func_caller!((*self.0).markUpdatedRows, start, end)
    }

    pub fn display(&self) -> Result<(), Error> {
        pd_func_caller!((*self.0).display)
    }

    pub fn set_draw_offset(&self, offset: ScreenVector) -> Result<(), Error> {
        pd_func_caller!((*self.0).setDrawOffset, offset.x, offset.y)
    }

    pub fn new_bitmap(&self, size: ScreenSize, bg_color: LCDColor) -> Result<Bitmap, Error> {
        let raw_bitmap = pd_func_caller!(
            (*self.0).newBitmap,
            size.width,
            size.height,
            bg_color.into()
        )?;
        anyhow::ensure!(
            raw_bitmap != ptr::null_mut(),
            "Null pointer returned from new_bitmap"
        );
        Ok(Bitmap::new(raw_bitmap))
    }

    pub fn load_bitmap(&self, path: &str) -> Result<Bitmap, Error> {
        let c_path = CString::new(path).map_err(Error::msg)?;
        let mut out_err: *const crankstart_sys::ctypes::c_char = ptr::null_mut();
        let raw_bitmap = pd_func_caller!((*self.0).loadBitmap, c_path.as_ptr(), &mut out_err)?;
        if raw_bitmap == ptr::null_mut() {
            if out_err != ptr::null_mut() {
                let err_msg = unsafe { CStr::from_ptr(out_err).to_string_lossy().into_owned() };
                Err(anyhow!(err_msg))
            } else {
                Err(anyhow!(
                    "load_bitmap failed without providing an error message"
                ))
            }
        } else {
            Ok(Bitmap::new(raw_bitmap))
        }
    }

    pub fn new_bitmap_table(&self, count: usize, size: ScreenSize) -> Result<BitmapTable, Error> {
        let raw_bitmap_table = pd_func_caller!(
            (*self.0).newBitmapTable,
            count as i32,
            size.width,
            size.height
        )?;

        Ok(BitmapTable::new(raw_bitmap_table))
    }

    pub fn load_bitmap_table(&self, path: &str) -> Result<BitmapTable, Error> {
        let c_path = CString::new(path).map_err(Error::msg)?;
        let mut out_err: *const crankstart_sys::ctypes::c_char = ptr::null_mut();
        let raw_bitmap_table =
            pd_func_caller!((*self.0).loadBitmapTable, c_path.as_ptr(), &mut out_err)?;
        if raw_bitmap_table == ptr::null_mut() {
            if out_err != ptr::null_mut() {
                let err_msg = unsafe { CStr::from_ptr(out_err).to_string_lossy().into_owned() };
                Err(anyhow!(err_msg))
            } else {
                Err(anyhow!(
                    "load_bitmap_table failed without providing an error message"
                ))
            }
        } else {
            Ok(BitmapTable::new(raw_bitmap_table))
        }
    }

    pub fn clear(&self, color: LCDColor) -> Result<(), Error> {
        pd_func_caller!((*self.0).clear, color.into())
    }

    pub fn draw_line(
        &self,
        target: OptionalBitmap,
        stencil: OptionalBitmap,
        p1: ScreenPoint,
        p2: ScreenPoint,
        width: i32,
        color: LCDColor,
        end_cap_style: LCDLineCapStyle,
        clip: LCDRect,
    ) -> Result<(), Error> {
        pd_func_caller!(
            (*self.0).drawLine,
            raw_bitmap(target),
            raw_bitmap(stencil),
            p1.x,
            p1.y,
            p2.x,
            p2.y,
            width,
            color.into(),
            end_cap_style,
            clip
        )
    }

    pub fn fill_triangle(
        &self,
        target: OptionalBitmap,
        stencil: OptionalBitmap,
        p1: ScreenPoint,
        p2: ScreenPoint,
        p3: ScreenPoint,
        color: LCDColor,
        clip: LCDRect,
    ) -> Result<(), Error> {
        pd_func_caller!(
            (*self.0).fillTriangle,
            raw_bitmap(target),
            raw_bitmap(stencil),
            p1.x,
            p1.y,
            p2.x,
            p2.y,
            p3.x,
            p3.y,
            color.into(),
            clip
        )
    }

    pub fn draw_rect(
        &self,
        target: OptionalBitmap,
        stencil: OptionalBitmap,
        rect: ScreenRect,
        color: LCDColor,
        clip: LCDRect,
    ) -> Result<(), Error> {
        pd_func_caller!(
            (*self.0).drawRect,
            raw_bitmap(target),
            raw_bitmap(stencil),
            rect.origin.x,
            rect.origin.y,
            rect.size.width,
            rect.size.height,
            color.into(),
            clip
        )
    }

    pub fn fill_rect(
        &self,
        target: OptionalBitmap,
        stencil: OptionalBitmap,
        rect: ScreenRect,
        color: LCDColor,
        clip: LCDRect,
    ) -> Result<(), Error> {
        pd_func_caller!(
            (*self.0).fillRect,
            raw_bitmap(target),
            raw_bitmap(stencil),
            rect.origin.x,
            rect.origin.y,
            rect.size.width,
            rect.size.height,
            color.into(),
            clip
        )
    }

    pub fn draw_ellipse(
        &self,
        target: OptionalBitmap,
        stencil: OptionalBitmap,
        center: ScreenPoint,
        size: ScreenSize,
        line_width: i32,
        start_angle: f32,
        end_angle: f32,
        color: LCDColor,
        clip: LCDRect,
    ) -> Result<(), Error> {
        pd_func_caller!(
            (*self.0).drawEllipse,
            raw_bitmap(target),
            raw_bitmap(stencil),
            center.x,
            center.y,
            size.width,
            size.height,
            line_width,
            start_angle,
            end_angle,
            color.into(),
            clip
        )
    }

    pub fn fill_ellipse(
        &self,
        target: OptionalBitmap,
        stencil: OptionalBitmap,
        center: ScreenPoint,
        size: ScreenSize,
        line_width: i32,
        start_angle: f32,
        end_angle: f32,
        color: LCDColor,
        clip: LCDRect,
    ) -> Result<(), Error> {
        pd_func_caller!(
            (*self.0).fillEllipse,
            raw_bitmap(target),
            raw_bitmap(stencil),
            center.x,
            center.y,
            size.width,
            size.height,
            start_angle,
            end_angle,
            color.into(),
            clip
        )
    }

    pub fn load_font(&self, path: &str) -> Result<Font, Error> {
        let c_path = CString::new(path).map_err(Error::msg)?;
        let font = pd_func_caller!((*self.0).loadFont, c_path.as_ptr(), ptr::null_mut())?;
        Font::new(font)
    }

    pub fn draw_text(
        &self,
        font: &Font,
        target: OptionalBitmap,
        stencil: OptionalBitmap,
        text: &str,
        position: ScreenPoint,
        mode: LCDBitmapDrawMode,
        tracking: i32,
        clip: LCDRect,
    ) -> Result<i32, Error> {
        let c_text = CString::new(text).map_err(Error::msg)?;
        pd_func_caller!(
            (*self.0).drawText,
            font.0,
            raw_bitmap(target),
            raw_bitmap(stencil),
            c_text.as_ptr() as *const core::ffi::c_void,
            text.len() as size_t,
            PDStringEncoding::kUTF8Encoding,
            position.x,
            position.y,
            mode.into(),
            tracking,
            clip,
        )
    }

    pub fn get_text_width(&self, font: &Font, text: &str, tracking: i32) -> Result<i32, Error> {
        let c_text = CString::new(text).map_err(Error::msg)?;
        pd_func_caller!(
            (*self.0).getTextWidth,
            font.0,
            c_text.as_ptr() as *const core::ffi::c_void,
            text.len() as size_t,
            PDStringEncoding::kUTF8Encoding,
            tracking,
        )
    }
}
