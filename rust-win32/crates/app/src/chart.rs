//! Direct2D resources only exist while the History page exists.
use hb_core::Reading;
use windows::Win32::{
    Foundation::HWND,
    Graphics::Direct2D::{Common::*, *},
};
use windows::{
    Win32::{Graphics::DirectWrite::*, UI::HiDpi::GetDpiForWindow},
    core::w,
};
use windows_numerics::Vector2;
pub struct Chart {
    target: ID2D1HwndRenderTarget,
    font: IDWriteTextFormat,
    scale: f32,
}
impl Chart {
    pub fn new(hwnd: HWND, width: u32, height: u32) -> windows::core::Result<Self> {
        unsafe {
            let factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let target = factory.CreateHwndRenderTarget(
                &D2D1_RENDER_TARGET_PROPERTIES {
                    r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
                    dpiX: 96.,
                    dpiY: 96.,
                    ..Default::default()
                },
                &D2D1_HWND_RENDER_TARGET_PROPERTIES {
                    hwnd,
                    pixelSize: D2D_SIZE_U { width, height },
                    presentOptions: D2D1_PRESENT_OPTIONS_NONE,
                },
            )?;
            let scale = GetDpiForWindow(hwnd) as f32 / 96.;
            let writer: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let font = writer.CreateTextFormat(
                w!("Segoe UI"),
                None,
                DWRITE_FONT_WEIGHT_NORMAL,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                13. * scale,
                w!("en-US"),
            )?;
            Ok(Self {
                target,
                font,
                scale,
            })
        }
    }
    pub fn paint(
        &self,
        width: u32,
        height: u32,
        points: &[Reading],
        since: i64,
        until: i64,
    ) -> windows::core::Result<()> {
        unsafe {
            self.target.Resize(&D2D_SIZE_U { width, height })?;
            self.target.BeginDraw();
            self.target.Clear(Some(&D2D1_COLOR_F {
                r: 0.98,
                g: 0.98,
                b: 0.98,
                a: 1.,
            }));
            let grid = self.target.CreateSolidColorBrush(
                &D2D1_COLOR_F {
                    r: 0.82,
                    g: 0.84,
                    b: 0.86,
                    a: 1.,
                },
                None,
            )?;
            let line = self.target.CreateSolidColorBrush(
                &D2D1_COLOR_F {
                    r: 0.10,
                    g: 0.52,
                    b: 0.38,
                    a: 1.,
                },
                None,
            )?;
            let left = 60. * self.scale;
            let right = width as f32 - 25. * self.scale;
            let top = 140. * self.scale;
            let bottom = height as f32 - 95. * self.scale;
            for level in [0, 25, 50, 75, 100] {
                let y = bottom - (bottom - top) * level as f32 / 100.;
                self.target.DrawLine(
                    Vector2 { X: left, Y: y },
                    Vector2 { X: right, Y: y },
                    &grid,
                    1.,
                    None,
                );
            }
            let label = self.target.CreateSolidColorBrush(
                &D2D1_COLOR_F {
                    r: 0.20,
                    g: 0.22,
                    b: 0.24,
                    a: 1.,
                },
                None,
            )?;
            let text = |text: &str, x: f32, y: f32, w: f32| {
                let chars: Vec<u16> = text.encode_utf16().collect();
                self.target.DrawText(
                    &chars,
                    &self.font,
                    &D2D_RECT_F {
                        left: x,
                        top: y,
                        right: x + w,
                        bottom: y + 24. * self.scale,
                    },
                    &label,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
            };
            for level in [0, 25, 50, 75, 100] {
                let y = bottom - (bottom - top) * level as f32 / 100.;
                text(
                    &format!("{level}%"),
                    8. * self.scale,
                    y - 10. * self.scale,
                    48. * self.scale,
                );
            }
            text(
                &format!("{} days ago", (until - since) / 86400),
                left,
                bottom + 12. * self.scale,
                150. * self.scale,
            );
            text(
                "Now",
                right - 36. * self.scale,
                bottom + 12. * self.scale,
                40. * self.scale,
            );
            if !points.iter().any(|r| r.level.is_some()) {
                text(
                    "No recorded readings in this interval.",
                    left + 40. * self.scale,
                    top + 50. * self.scale,
                    420. * self.scale,
                );
            }
            let mut previous = None;
            for r in points {
                if !r.online() {
                    previous = None;
                    continue;
                }
                let Some(level) = r.level else {
                    previous = None;
                    continue;
                };
                let p = Vector2 {
                    X: left
                        + (right - left) * (r.timestamp - since) as f32
                            / (until - since).max(1) as f32,
                    Y: bottom - (bottom - top) * level as f32 / 100.,
                };
                if let Some(a) = previous {
                    self.target.DrawLine(a, p, &line, 2., None);
                }
                self.target.FillEllipse(
                    &D2D1_ELLIPSE {
                        point: p,
                        radiusX: 2. * self.scale,
                        radiusY: 2. * self.scale,
                    },
                    &line,
                );
                previous = Some(p);
            }
            self.target.EndDraw(None, None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::UI::WindowsAndMessaging::*;
    #[test]
    fn history_target_can_paint_and_release_repeatedly() {
        let _guard = crate::ui::NATIVE_TEST_LOCK.lock().unwrap();
        unsafe {
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("STATIC"),
                w!("Halo chart test"),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                840,
                820,
                None,
                None,
                None,
                None,
            )
            .unwrap();
            for _ in 0..5 {
                let chart = Chart::new(hwnd, 840, 820).unwrap();
                chart.paint(840, 820, &[], 0, 86400).unwrap();
                drop(chart);
            }
            DestroyWindow(hwnd).unwrap();
        }
    }
}
