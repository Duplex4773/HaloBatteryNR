//! Dashboard-only colors and documented native-control painting. Control input,
//! focus, selection and accessibility remain with the Windows control classes.
use windows::{
    Win32::{
        Foundation::*,
        Graphics::{Dwm::*, Gdi::*},
        UI::{
            Controls::*,
            HiDpi::GetDpiForWindow,
            Input::KeyboardAndMouse::{GetFocus, IsWindowEnabled},
            Shell::*,
            WindowsAndMessaging::*,
        },
    },
    core::{PCWSTR, w},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    pub background: COLORREF,
    pub surface: COLORREF,
    pub text: COLORREF,
    pub disabled: COLORREF,
    pub border: COLORREF,
    pub accent: COLORREF,
    pub selection: COLORREF,
    pub selection_text: COLORREF,
    pub dark: bool,
    pub high_contrast: bool,
}
const fn rgb(r: u32, g: u32, b: u32) -> COLORREF {
    COLORREF(r | (g << 8) | (b << 16))
}
impl Palette {
    pub fn new(dark: bool, high_contrast: bool) -> Self {
        if high_contrast {
            unsafe {
                return Self {
                    background: COLORREF(GetSysColor(COLOR_WINDOW)),
                    surface: COLORREF(GetSysColor(COLOR_WINDOW)),
                    text: COLORREF(GetSysColor(COLOR_WINDOWTEXT)),
                    disabled: COLORREF(GetSysColor(COLOR_GRAYTEXT)),
                    border: COLORREF(GetSysColor(COLOR_WINDOWTEXT)),
                    accent: COLORREF(GetSysColor(COLOR_HIGHLIGHT)),
                    selection: COLORREF(GetSysColor(COLOR_HIGHLIGHT)),
                    selection_text: COLORREF(GetSysColor(COLOR_HIGHLIGHTTEXT)),
                    dark: false,
                    high_contrast,
                };
            }
        }
        if dark {
            Self {
                background: rgb(32, 32, 32),
                surface: rgb(45, 45, 45),
                text: rgb(242, 242, 242),
                disabled: rgb(160, 160, 160),
                border: rgb(112, 112, 112),
                accent: rgb(91, 218, 168),
                selection: rgb(45, 92, 119),
                selection_text: rgb(255, 255, 255),
                dark,
                high_contrast,
            }
        } else {
            Self {
                background: rgb(250, 250, 250),
                surface: rgb(255, 255, 255),
                text: rgb(32, 35, 38),
                disabled: rgb(109, 109, 109),
                border: rgb(185, 190, 195),
                accent: rgb(25, 133, 97),
                selection: rgb(0, 120, 215),
                selection_text: rgb(255, 255, 255),
                dark,
                high_contrast,
            }
        }
    }
    pub fn d2d(color: COLORREF) -> windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F {
        windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F {
            r: (color.0 & 255) as f32 / 255.,
            g: ((color.0 >> 8) & 255) as f32 / 255.,
            b: ((color.0 >> 16) & 255) as f32 / 255.,
            a: 1.,
        }
    }
}
/// Use the documented DC stock brush for a single fill/frame inside a saved DC.
/// The brush belongs to Windows; RestoreDC restores its selected color.
pub(crate) fn color_brush(hdc: HDC, color: COLORREF) -> HBRUSH {
    unsafe {
        SetDCBrushColor(hdc, color);
        HBRUSH(GetStockObject(DC_BRUSH).0)
    }
}
struct Brush(HBRUSH);
impl Brush {
    fn new(color: COLORREF) -> Self {
        Self(unsafe { CreateSolidBrush(color) })
    }
}
impl Drop for Brush {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(self.0.into());
        }
    }
}
pub struct DashboardTheme {
    pub palette: Palette,
    background: Brush,
    surface: Brush,
}
impl DashboardTheme {
    pub fn new(dark: bool, high_contrast: bool) -> Self {
        let palette = Palette::new(dark, high_contrast);
        Self {
            palette,
            background: Brush::new(palette.background),
            surface: Brush::new(palette.surface),
        }
    }
    pub fn background_brush(&self) -> HBRUSH {
        self.background.0
    }
    pub fn apply_window(&self, hwnd: HWND) {
        unsafe {
            let dark = i32::from(self.palette.dark);
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                &dark as *const _ as _,
                size_of::<i32>() as u32,
            );
            let _ = InvalidateRect(Some(hwnd), None, true);
        }
    }
    pub fn apply_control(&self, hwnd: HWND) {
        unsafe {
            let class = class_name(hwnd);
            if class == "Button" || class == "ComboBox" {
                let mut data = 0usize;
                if GetWindowSubclass(hwnd, Some(control_proc), SUBCLASS_ID, Some(&mut data))
                    .as_bool()
                {
                    *(data as *mut Palette) = self.palette;
                } else {
                    let data = Box::into_raw(Box::new(self.palette));
                    if !SetWindowSubclass(hwnd, Some(control_proc), SUBCLASS_ID, data as usize)
                        .as_bool()
                    {
                        drop(Box::from_raw(data));
                    }
                }
            }
            // Empty theme disables visual-style white surfaces for edit/list/combo.
            // Buttons retain their native theme in light/high-contrast mode.
            if class != "Button" {
                let theme = if self.palette.dark {
                    w!("")
                } else {
                    PCWSTR::null()
                };
                let _ = SetWindowTheme(hwnd, theme, theme);
            }
            if class == "ComboBox" {
                let height = (22 * GetDpiForWindow(hwnd) / 96) as isize;
                SendMessageW(
                    hwnd,
                    CB_SETITEMHEIGHT,
                    Some(WPARAM(usize::MAX)),
                    Some(LPARAM(height)),
                );
                SendMessageW(
                    hwnd,
                    CB_SETITEMHEIGHT,
                    Some(WPARAM(0)),
                    Some(LPARAM(height)),
                );
            }
            let _ = InvalidateRect(Some(hwnd), None, true);
        }
    }
    pub fn control_colors(&self, message: u32, hdc: HDC, child: HWND) -> Option<LRESULT> {
        let surface = matches!(message, WM_CTLCOLOREDIT | WM_CTLCOLORLISTBOX)
            || (message == WM_CTLCOLORSTATIC && unsafe { class_name(child) } == "Edit");
        if !surface && !matches!(message, WM_CTLCOLORSTATIC | WM_CTLCOLORBTN) {
            return None;
        }
        unsafe {
            SetTextColor(
                hdc,
                if IsWindowEnabled(child).as_bool() {
                    self.palette.text
                } else {
                    self.palette.disabled
                },
            );
            SetBkColor(
                hdc,
                if surface {
                    self.palette.surface
                } else {
                    self.palette.background
                },
            );
            SetBkMode(hdc, TRANSPARENT);
        }
        Some(LRESULT(if surface {
            self.surface.0.0
        } else {
            self.background.0.0
        } as isize))
    }
    pub fn draw_item(&self, lparam: LPARAM) -> Option<LRESULT> {
        if lparam.0 == 0 {
            return None;
        }
        unsafe {
            let item = &*(lparam.0 as *const DRAWITEMSTRUCT);
            if item.CtlType != ODT_COMBOBOX {
                return None;
            }
            let saved = SaveDC(item.hDC);
            let selected = item.itemState.0 & ODS_SELECTED.0 != 0
                && item.itemState.0 & ODS_COMBOBOXEDIT.0 == 0;
            let bg = if selected {
                self.palette.selection
            } else {
                self.palette.surface
            };
            let fg = if item.itemState.0 & ODS_DISABLED.0 != 0 {
                self.palette.disabled
            } else if selected {
                self.palette.selection_text
            } else {
                self.palette.text
            };
            FillRect(item.hDC, &item.rcItem, color_brush(item.hDC, bg));
            if item.itemID != u32::MAX {
                let len = SendMessageW(
                    item.hwndItem,
                    CB_GETLBTEXTLEN,
                    Some(WPARAM(item.itemID as usize)),
                    None,
                )
                .0;
                if len >= 0 {
                    let mut text = vec![0u16; len as usize + 1];
                    SendMessageW(
                        item.hwndItem,
                        CB_GETLBTEXT,
                        Some(WPARAM(item.itemID as usize)),
                        Some(LPARAM(text.as_mut_ptr() as isize)),
                    );
                    text.truncate(len as usize);
                    let mut rect = item.rcItem;
                    rect.left += 6;
                    SetTextColor(item.hDC, fg);
                    SetBkMode(item.hDC, TRANSPARENT);
                    DrawTextW(
                        item.hDC,
                        &mut text,
                        &mut rect,
                        DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX,
                    );
                }
            }
            if item.itemState.0 & ODS_FOCUS.0 != 0 && item.itemState.0 & ODS_NOFOCUSRECT.0 == 0 {
                let _ = DrawFocusRect(item.hDC, &item.rcItem);
            }
            let _ = RestoreDC(item.hDC, saved);
        }
        Some(LRESULT(1))
    }
}
const SUBCLASS_ID: usize = 0x48425448;
unsafe fn class_name(hwnd: HWND) -> String {
    unsafe {
        let mut name = [0u16; 64];
        let len = GetClassNameW(hwnd, &mut name);
        String::from_utf16_lossy(&name[..len.max(0) as usize])
    }
}
unsafe extern "system" fn control_proc(
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
    _: usize,
    data: usize,
) -> LRESULT {
    std::panic::catch_unwind(|| unsafe { control_message(hwnd, msg, wp, lp, data) })
        .unwrap_or(LRESULT(0))
}
unsafe fn control_message(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM, data: usize) -> LRESULT {
    unsafe {
        if msg == WM_NCDESTROY {
            let _ = RemoveWindowSubclass(hwnd, Some(control_proc), SUBCLASS_ID);
            drop(Box::from_raw(data as *mut Palette));
            return DefSubclassProc(hwnd, msg, wp, lp);
        }
        let palette = *(data as *const Palette);
        if matches!(msg, WM_PAINT | WM_PRINTCLIENT | WM_PRINT) && palette.dark {
            let printing = matches!(msg, WM_PRINTCLIENT | WM_PRINT);
            let class = class_name(hwnd);
            let combo = class == "ComboBox";
            if combo {
                let _ = DefSubclassProc(hwnd, msg, wp, lp);
            }
            let mut ps = PAINTSTRUCT::default();
            let hdc = if printing {
                HDC(wp.0 as *mut _)
            } else if combo {
                GetDC(Some(hwnd))
            } else {
                BeginPaint(hwnd, &mut ps)
            };
            let saved = SaveDC(hdc);
            let mut rect = RECT::default();
            let _ = GetClientRect(hwnd, &mut rect);
            if combo {
                FrameRect(hdc, &rect, color_brush(hdc, palette.border));
                rect.left = rect.right - (22 * GetDpiForWindow(hwnd) / 96) as i32;
                rect.top += 1;
                rect.bottom -= 1;
                rect.right -= 1;
                FillRect(hdc, &rect, color_brush(hdc, palette.surface));
                let mut arrow: Vec<u16> = "▾".encode_utf16().collect();
                SetTextColor(hdc, palette.text);
                SetBkMode(hdc, TRANSPARENT);
                DrawTextW(
                    hdc,
                    &mut arrow,
                    &mut rect,
                    DT_CENTER | DT_SINGLELINE | DT_VCENTER,
                );
            } else {
                let style = GetWindowLongW(hwnd, GWL_STYLE) as u32;
                let check = style & BS_TYPEMASK as u32 == BS_AUTOCHECKBOX as u32;
                let state = SendMessageW(hwnd, BM_GETSTATE, None, None).0 as u32;
                FillRect(
                    hdc,
                    &rect,
                    color_brush(
                        hdc,
                        if check {
                            palette.background
                        } else {
                            palette.surface
                        },
                    ),
                );
                let mut text_rect = rect;
                if check {
                    let size = (16 * GetDpiForWindow(hwnd) / 96) as i32;
                    let mark = RECT {
                        left: 1,
                        top: (rect.bottom - size) / 2,
                        right: 1 + size,
                        bottom: (rect.bottom + size) / 2,
                    };
                    FillRect(hdc, &mark, color_brush(hdc, palette.surface));
                    FrameRect(hdc, &mark, color_brush(hdc, palette.border));
                    if SendMessageW(hwnd, BM_GETCHECK, None, None).0 == BST_CHECKED.0 as isize {
                        let mut mark_rect = mark;
                        let mut tick: Vec<u16> = "✓".encode_utf16().collect();
                        SetTextColor(hdc, palette.accent);
                        SetBkMode(hdc, TRANSPARENT);
                        DrawTextW(
                            hdc,
                            &mut tick,
                            &mut mark_rect,
                            DT_CENTER | DT_VCENTER | DT_SINGLELINE,
                        );
                    }
                    text_rect.left = mark.right + 7;
                } else {
                    FrameRect(
                        hdc,
                        &rect,
                        color_brush(
                            hdc,
                            if state & BST_PUSHED != 0 {
                                palette.background
                            } else {
                                palette.border
                            },
                        ),
                    );
                }
                let font = SendMessageW(hwnd, WM_GETFONT, None, None);
                if font.0 != 0 {
                    SelectObject(hdc, HGDIOBJ(font.0 as _));
                }
                let mut text = vec![0u16; GetWindowTextLengthW(hwnd).max(0) as usize + 1];
                let len = GetWindowTextW(hwnd, &mut text);
                text.truncate(len.max(0) as usize);
                SetTextColor(
                    hdc,
                    if IsWindowEnabled(hwnd).as_bool() {
                        palette.text
                    } else {
                        palette.disabled
                    },
                );
                SetBkMode(hdc, TRANSPARENT);
                let ui = SendMessageW(hwnd, WM_QUERYUISTATE, None, None).0 as u32;
                let mut flags = DT_SINGLELINE | DT_VCENTER;
                if !check {
                    flags |= DT_CENTER;
                }
                if ui & UISF_HIDEACCEL != 0 {
                    flags |= DT_HIDEPREFIX;
                }
                DrawTextW(hdc, &mut text, &mut text_rect, flags);
                if GetFocus() == hwnd && ui & UISF_HIDEFOCUS == 0 {
                    text_rect.left += 2;
                    text_rect.top += 2;
                    text_rect.right -= 2;
                    text_rect.bottom -= 2;
                    let _ = DrawFocusRect(hdc, &text_rect);
                }
            }
            let _ = RestoreDC(hdc, saved);
            if printing {
                // The caller owns the print DC.
            } else if combo {
                ReleaseDC(Some(hwnd), hdc);
            } else {
                let _ = EndPaint(hwnd, &ps);
            }
            return LRESULT(0);
        }
        let result = DefSubclassProc(hwnd, msg, wp, lp);
        if palette.dark
            && matches!(
                msg,
                BM_SETCHECK
                    | BM_SETSTATE
                    | WM_ENABLE
                    | WM_SETTEXT
                    | WM_UPDATEUISTATE
                    | WM_SETFOCUS
                    | WM_KILLFOCUS
                    | CB_SETCURSEL
                    | CB_RESETCONTENT
                    | CB_ADDSTRING
                    | CB_DELETESTRING
            )
        {
            let _ = InvalidateRect(Some(hwnd), None, true);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selected_combo_text_and_static_colors_use_the_dashboard_palette() {
        let _guard = crate::ui::NATIVE_TEST_LOCK.lock().unwrap();
        unsafe {
            let combo = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("COMBOBOX"),
                w!(""),
                WS_POPUP
                    | WINDOW_STYLE((CBS_DROPDOWNLIST | CBS_OWNERDRAWFIXED | CBS_HASSTRINGS) as u32),
                0,
                0,
                240,
                200,
                None,
                None,
                None,
                None,
            )
            .unwrap();
            SendMessageW(
                combo,
                CB_ADDSTRING,
                None,
                Some(LPARAM(w!("Test device").as_ptr() as isize)),
            );
            SendMessageW(combo, CB_SETCURSEL, Some(WPARAM(0)), None);
            let label = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("STATIC"),
                w!("Ready"),
                WS_POPUP,
                0,
                0,
                240,
                40,
                None,
                None,
                None,
                None,
            )
            .unwrap();
            let screen = GetDC(None);
            let dc = CreateCompatibleDC(Some(screen));
            let bitmap = CreateCompatibleBitmap(screen, 240, 40);
            let previous = SelectObject(dc, bitmap.into());
            for (dark, hc) in [(false, false), (true, false), (true, true)] {
                let theme = DashboardTheme::new(dark, hc);
                let rect = RECT {
                    left: 0,
                    top: 0,
                    right: 240,
                    bottom: 40,
                };
                for (state, expected) in [
                    (
                        ODS_FLAGS(ODS_SELECTED.0 | ODS_COMBOBOXEDIT.0),
                        theme.palette.surface,
                    ),
                    (ODS_SELECTED, theme.palette.selection),
                ] {
                    let item = DRAWITEMSTRUCT {
                        CtlType: ODT_COMBOBOX,
                        itemID: 0,
                        itemState: state,
                        hwndItem: combo,
                        hDC: dc,
                        rcItem: rect,
                        ..Default::default()
                    };
                    assert_eq!(
                        theme.draw_item(LPARAM(&item as *const _ as isize)),
                        Some(LRESULT(1))
                    );
                    assert_eq!(GetPixel(dc, 235, 35), expected);
                    assert!((0..40).any(|y| (6..150).any(|x| GetPixel(dc, x, y) != expected)));
                }
                let brush = theme.control_colors(WM_CTLCOLORSTATIC, dc, label).unwrap();
                assert_eq!(GetTextColor(dc), theme.palette.text);
                assert_eq!(GetBkColor(dc), theme.palette.background);
                FillRect(dc, &rect, HBRUSH(brush.0 as *mut _));
                assert_eq!(GetPixel(dc, 235, 35), theme.palette.background);
            }
            SelectObject(dc, previous);
            let _ = DeleteObject(bitmap.into());
            let _ = DeleteDC(dc);
            ReleaseDC(None, screen);
            DestroyWindow(label).unwrap();
            DestroyWindow(combo).unwrap();
        }
    }
    #[test]
    fn dark_combo_print_keeps_arrow_surface_dark() {
        let _guard = crate::ui::NATIVE_TEST_LOCK.lock().unwrap();
        unsafe {
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("COMBOBOX"),
                w!(""),
                WS_POPUP | WINDOW_STYLE(CBS_DROPDOWNLIST as u32),
                0,
                0,
                240,
                200,
                None,
                None,
                None,
                None,
            )
            .unwrap();
            let theme = DashboardTheme::new(true, false);
            theme.apply_control(hwnd);
            let mut rect = RECT::default();
            GetClientRect(hwnd, &mut rect).unwrap();
            let screen = GetDC(None);
            let dc = CreateCompatibleDC(Some(screen));
            let bitmap = CreateCompatibleBitmap(screen, rect.right, rect.bottom);
            let previous = SelectObject(dc, bitmap.into());
            for message in [WM_PRINT, WM_PRINTCLIENT] {
                SendMessageW(
                    hwnd,
                    message,
                    Some(WPARAM(dc.0 as usize)),
                    Some(LPARAM(
                        (PRF_CLIENT | PRF_CHILDREN | PRF_ERASEBKGND) as isize,
                    )),
                );
                assert_eq!(GetPixel(dc, rect.right - 4, 4), theme.palette.surface);
            }
            SelectObject(dc, previous);
            let _ = DeleteObject(bitmap.into());
            let _ = DeleteDC(dc);
            ReleaseDC(None, screen);
            DestroyWindow(hwnd).unwrap();
        }
    }
    #[test]
    fn dark_buttons_print_dark_surfaces_and_readable_labels() {
        let _guard = crate::ui::NATIVE_TEST_LOCK.lock().unwrap();
        unsafe {
            for style in [0, BS_AUTOCHECKBOX] {
                let hwnd = CreateWindowExW(
                    WINDOW_EX_STYLE::default(),
                    w!("BUTTON"),
                    w!("Enable alerts"),
                    WS_POPUP | WINDOW_STYLE(style as u32),
                    0,
                    0,
                    240,
                    40,
                    None,
                    None,
                    None,
                    None,
                )
                .unwrap();
                let theme = DashboardTheme::new(true, false);
                theme.apply_control(hwnd);
                let screen = GetDC(None);
                let dc = CreateCompatibleDC(Some(screen));
                let bitmap = CreateCompatibleBitmap(screen, 240, 40);
                let previous = SelectObject(dc, bitmap.into());
                SendMessageW(
                    hwnd,
                    WM_PRINTCLIENT,
                    Some(WPARAM(dc.0 as usize)),
                    Some(LPARAM(PRF_CLIENT as isize)),
                );
                let expected = if style == BS_AUTOCHECKBOX {
                    theme.palette.background
                } else {
                    theme.palette.surface
                };
                assert_eq!(GetPixel(dc, 235, 35), expected);
                assert!((0..40).any(|y| (24..220).any(|x| {
                    let c = GetPixel(dc, x, y);
                    c.0 & 255 > 180 && (c.0 >> 8) & 255 > 180 && (c.0 >> 16) & 255 > 180
                })));
                SelectObject(dc, previous);
                let _ = DeleteObject(bitmap.into());
                let _ = DeleteDC(dc);
                ReleaseDC(None, screen);
                DestroyWindow(hwnd).unwrap();
            }
        }
    }
    #[test]
    fn native_controls_keep_state_across_theme_changes_and_release_subclasses() {
        let _guard = crate::ui::NATIVE_TEST_LOCK.lock().unwrap();
        unsafe {
            for _ in 0..10 {
                let hwnd = CreateWindowExW(
                    WINDOW_EX_STYLE::default(),
                    w!("BUTTON"),
                    w!("Enable &alerts"),
                    WS_OVERLAPPEDWINDOW | WINDOW_STYLE(BS_AUTOCHECKBOX as u32),
                    0,
                    0,
                    240,
                    60,
                    None,
                    None,
                    None,
                    None,
                )
                .unwrap();
                SendMessageW(
                    hwnd,
                    BM_SETCHECK,
                    Some(WPARAM(BST_CHECKED.0 as usize)),
                    None,
                );
                for (dark, hc) in [(false, false), (true, false), (true, true), (false, false)] {
                    let theme = DashboardTheme::new(dark, hc);
                    theme.apply_control(hwnd);
                    let _ = UpdateWindow(hwnd);
                    assert_eq!(
                        SendMessageW(hwnd, BM_GETCHECK, None, None).0,
                        BST_CHECKED.0 as isize
                    );
                    let mut data = 0;
                    assert!(
                        GetWindowSubclass(hwnd, Some(control_proc), SUBCLASS_ID, Some(&mut data))
                            .as_bool()
                    );
                    assert_eq!(*(data as *const Palette), theme.palette);
                }
                DestroyWindow(hwnd).unwrap();
            }
        }
    }
    #[test]
    fn palettes_and_resources_follow_preferences() {
        let _guard = crate::ui::NATIVE_TEST_LOCK.lock().unwrap();
        assert!(Palette::new(true, false).dark);
        assert!(!Palette::new(true, true).dark);
        assert_eq!(Palette::new(false, true).background, unsafe {
            COLORREF(GetSysColor(COLOR_WINDOW))
        });
        assert_ne!(
            Palette::new(true, false).text,
            Palette::new(true, false).background
        );
        for _ in 0..100 {
            for (dark, hc) in [(false, false), (true, false), (true, true)] {
                let theme = DashboardTheme::new(dark, hc);
                assert!(!theme.background_brush().0.is_null());
            }
        }
    }
}
