use eframe::egui::{self, Color32, Pos2, Rect, Response, Stroke};

pub const ACCENT: Color32 = Color32::from_rgb(216, 176, 140);
pub const PANEL: Color32 = Color32::from_rgb(24, 25, 28);
pub const BACKGROUND: Color32 = Color32::from_rgb(12, 13, 15);
pub const SECONDARY: Color32 = Color32::from_rgb(166, 168, 174);
const FOREGROUND: Color32 = Color32::from_rgb(237, 237, 239);

pub fn apply_theme(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Dark);
    let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
    style.visuals = egui::Visuals::dark();
    style.visuals.panel_fill = PANEL;
    style.visuals.window_fill = PANEL;
    style.visuals.extreme_bg_color = BACKGROUND;
    style.visuals.override_text_color = Some(FOREGROUND);
    style.visuals.selection.bg_fill = ACCENT;
    style.visuals.selection.stroke = Stroke::new(1.0, BACKGROUND);
    style.visuals.slider_trailing_fill = true;
    style.visuals.handle_shape = egui::style::HandleShape::Circle;
    style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(43, 44, 49);
    style.visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(35, 36, 40);
    style.visuals.widgets.inactive.bg_stroke = Stroke::NONE;
    style.visuals.widgets.inactive.fg_stroke = Stroke::new(1.5, FOREGROUND);
    style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(61, 62, 68);
    style.visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(45, 46, 51);
    style.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, SECONDARY);
    style.visuals.widgets.hovered.fg_stroke = Stroke::new(1.5, FOREGROUND);
    style.visuals.widgets.active.bg_fill = ACCENT;
    style.visuals.widgets.active.weak_bg_fill = Color32::from_rgb(72, 61, 53);
    style.visuals.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
    style.visuals.widgets.active.fg_stroke = Stroke::new(1.5, FOREGROUND);
    style.visuals.widgets.noninteractive.bg_stroke =
        Stroke::new(1.0, Color32::from_rgb(43, 44, 49));
    style.visuals.widgets.noninteractive.corner_radius = egui::CornerRadius::same(8);
    style.visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(8);
    style.visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(8);
    style.visuals.widgets.active.corner_radius = egui::CornerRadius::same(8);
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(12.0, 8.0);
    style.spacing.interact_size.y = 32.0;
    style
        .text_styles
        .insert(egui::TextStyle::Body, egui::FontId::proportional(14.0));
    style
        .text_styles
        .insert(egui::TextStyle::Button, egui::FontId::proportional(14.0));
    style
        .text_styles
        .insert(egui::TextStyle::Heading, egui::FontId::proportional(24.0));
    style
        .text_styles
        .insert(egui::TextStyle::Small, egui::FontId::proportional(12.0));
    ctx.set_style_of(egui::Theme::Dark, style);
    if let Ok(bytes) = std::fs::read(r"C:\Windows\Fonts\segoeui.ttf") {
        let mut fonts = egui::FontDefinitions::default();
        fonts
            .font_data
            .insert("Segoe UI".into(), egui::FontData::from_owned(bytes).into());
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .insert(0, "Segoe UI".into());
        ctx.set_fonts(fonts);
    }
}

#[derive(Clone, Copy)]
pub enum Icon {
    Play,
    Pause,
    Back,
    Forward,
    Volume,
    Muted,
    Fullscreen,
    ExitFullscreen,
    Music,
    PreviousFile,
    NextFile,
}

pub fn paint_icon(painter: &egui::Painter, rect: Rect, icon: Icon, color: Color32) {
    let point = |x: f32, y: f32| {
        Pos2::new(
            rect.left() + x * rect.width() / 24.0,
            rect.top() + y * rect.height() / 24.0,
        )
    };
    let stroke = Stroke::new(rect.width() / 14.0, color);
    let line = |points: &[(f32, f32)]| {
        painter.add(egui::Shape::line(
            points.iter().map(|&(x, y)| point(x, y)).collect(),
            stroke,
        ));
    };
    match icon {
        Icon::Play => {
            painter.add(egui::Shape::convex_polygon(
                vec![point(8.0, 5.0), point(19.0, 12.0), point(8.0, 19.0)],
                color,
                Stroke::NONE,
            ));
        }
        Icon::Pause => {
            for x in [7.0, 14.0] {
                painter.rect_filled(
                    Rect::from_min_max(point(x, 5.0), point(x + 3.0, 19.0)),
                    1.0,
                    color,
                );
            }
        }
        Icon::Back => {
            line(&[(11.0, 5.0), (5.0, 12.0), (11.0, 19.0)]);
            line(&[(19.0, 5.0), (13.0, 12.0), (19.0, 19.0)]);
        }
        Icon::Forward => {
            line(&[(5.0, 5.0), (11.0, 12.0), (5.0, 19.0)]);
            line(&[(13.0, 5.0), (19.0, 12.0), (13.0, 19.0)]);
        }
        Icon::Volume | Icon::Muted => {
            painter.add(egui::Shape::convex_polygon(
                vec![
                    point(3.0, 9.0),
                    point(7.0, 9.0),
                    point(12.0, 5.0),
                    point(12.0, 19.0),
                    point(7.0, 15.0),
                    point(3.0, 15.0),
                ],
                color,
                Stroke::NONE,
            ));
            if matches!(icon, Icon::Muted) {
                line(&[(16.0, 9.0), (22.0, 15.0)]);
                line(&[(16.0, 15.0), (22.0, 9.0)]);
            } else {
                line(&[(16.0, 8.0), (18.0, 10.0), (18.0, 14.0), (16.0, 16.0)]);
                line(&[(19.0, 5.0), (22.0, 8.0), (22.0, 16.0), (19.0, 19.0)]);
            }
        }
        Icon::Fullscreen => {
            for points in [
                [(4.0, 9.0), (4.0, 4.0), (9.0, 4.0)],
                [(15.0, 4.0), (20.0, 4.0), (20.0, 9.0)],
                [(20.0, 15.0), (20.0, 20.0), (15.0, 20.0)],
                [(9.0, 20.0), (4.0, 20.0), (4.0, 15.0)],
            ] {
                line(&points);
            }
        }
        Icon::ExitFullscreen => {
            for points in [
                [(4.0, 9.0), (9.0, 9.0), (9.0, 4.0)],
                [(15.0, 4.0), (15.0, 9.0), (20.0, 9.0)],
                [(20.0, 15.0), (15.0, 15.0), (15.0, 20.0)],
                [(9.0, 20.0), (9.0, 15.0), (4.0, 15.0)],
            ] {
                line(&points);
            }
        }
        Icon::PreviousFile => {
            line(&[(5.0, 5.0), (5.0, 19.0)]);
            painter.add(egui::Shape::convex_polygon(
                vec![point(18.0, 5.0), point(8.0, 12.0), point(18.0, 19.0)],
                color,
                Stroke::NONE,
            ));
        }
        Icon::NextFile => {
            line(&[(19.0, 5.0), (19.0, 19.0)]);
            painter.add(egui::Shape::convex_polygon(
                vec![point(6.0, 5.0), point(16.0, 12.0), point(6.0, 19.0)],
                color,
                Stroke::NONE,
            ));
        }
        Icon::Music => {
            line(&[(9.0, 17.0), (9.0, 6.0), (19.0, 4.0), (19.0, 15.0)]);
            painter.circle_filled(point(6.0, 18.0), rect.width() / 7.0, color);
            painter.circle_filled(point(16.0, 16.0), rect.width() / 7.0, color);
        }
    }
}

pub fn icon_button(ui: &mut egui::Ui, icon: Icon, label: &str, primary: bool) -> Response {
    let size = if primary { 44.0 } else { 36.0 };
    let mut button = egui::Button::new("").min_size(egui::vec2(size, size));
    if primary {
        button = button.fill(if ui.is_enabled() {
            ACCENT
        } else {
            Color32::from_rgb(43, 44, 49)
        });
    } else {
        button = button.frame_when_inactive(false);
    }
    let response = ui.add(button).on_hover_text(label);
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    let color = if !ui.is_enabled() {
        SECONDARY.gamma_multiply(0.45)
    } else if primary {
        BACKGROUND
    } else {
        ui.style().interact(&response).fg_stroke.color
    };
    paint_icon(
        ui.painter(),
        Rect::from_center_size(response.rect.center(), egui::vec2(22.0, 22.0)),
        icon,
        color,
    );
    response
}

pub fn timeline(ui: &mut egui::Ui, position: &mut f64, duration: f64) -> Response {
    let width = ui.available_width();
    ui.scope(|ui| {
        ui.spacing_mut().slider_width = width;
        ui.spacing_mut().slider_rail_height = 4.0;
        ui.visuals_mut().widgets.inactive.corner_radius = egui::CornerRadius::same(2);
        ui.spacing_mut().interact_size.y = 24.0;
        ui.add(
            egui::Slider::new(position, 0.0..=duration.max(0.01))
                .show_value(false)
                .trailing_fill(true)
                .smart_aim(false),
        )
    })
    .inner
}

pub fn navigation_layout(content: Rect, visible: bool) -> (Rect, [Rect; 2]) {
    if !visible || content.width() < 160.0 || content.height() < 48.0 {
        return (content, [Rect::NOTHING; 2]);
    }
    let video = content.shrink2(egui::vec2(56.0, 0.0));
    let buttons = [
        Rect::from_center_size(
            egui::pos2(content.left() + 28.0, content.center().y),
            egui::vec2(40.0, 48.0),
        ),
        Rect::from_center_size(
            egui::pos2(content.right() - 28.0, content.center().y),
            egui::vec2(40.0, 48.0),
        ),
    ];
    (video, buttons)
}

pub fn file_button(
    ui: &mut egui::Ui,
    rect: Rect,
    previous: bool,
    enabled: bool,
    label: &str,
) -> Response {
    let response = ui
        .push_id(("file-navigation", previous), |ui| {
            ui.add_enabled_ui(enabled, |ui| {
                ui.put(rect, egui::Button::new("").fill(PANEL).corner_radius(10))
            })
            .inner
        })
        .inner;
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    let color = if enabled {
        ui.style().interact(&response).fg_stroke.color
    } else {
        SECONDARY.gamma_multiply(0.4)
    };
    let icon = if previous {
        Icon::PreviousFile
    } else {
        Icon::NextFile
    };
    paint_icon(
        ui.painter(),
        Rect::from_center_size(rect.center(), egui::vec2(24.0, 24.0)),
        icon,
        color,
    );
    if egui::Tooltip::should_show_tooltip(&response, false) {
        // The native video HWND sits above egui, so keep tooltips in the header.
        let anchor = egui::pos2(rect.center().x, (ui.max_rect().top() - 48.0).max(0.0));
        egui::Tooltip::always_open(ui.ctx().clone(), response.layer_id, response.id, anchor)
            .gap(0.0)
            .width(280.0)
            .show(|ui| {
                ui.add(egui::Label::new(label).truncate());
            });
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    fn frame(
        ctx: &egui::Context,
        width: f32,
        position: &mut f64,
        events: Vec<egui::Event>,
        enabled: bool,
    ) -> Response {
        let mut response = None;
        ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(width, 420.0))),
                events,
                ..Default::default()
            },
            |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    ui.add_enabled_ui(enabled, |ui| {
                        let available = ui.available_width();
                        let slider = timeline(ui, position, 600.0);
                        assert!((slider.rect.width() - available).abs() <= 1.0);
                        assert!(slider.rect.right() <= ui.max_rect().right() + 1.0);
                        response = Some(slider);
                    });
                });
            },
        )
        .drop_without_applying_deltas();
        response.unwrap()
    }

    #[test]
    fn timeline_fills_small_and_large_windows() {
        for width in [640.0, 960.0, 1440.0] {
            let ctx = egui::Context::default();
            let mut position = 0.0;
            frame(&ctx, width, &mut position, Vec::new(), true);
            let response = frame(&ctx, width, &mut position, Vec::new(), true);
            assert!(response.rect.width() > width * 0.95);
        }
    }

    #[test]
    fn timeline_drag_seeks_across_the_track_and_releases() {
        let ctx = egui::Context::default();
        let mut position = 0.0;
        let rect = frame(&ctx, 960.0, &mut position, Vec::new(), true).rect;
        let start = egui::pos2(rect.left() + rect.width() * 0.25, rect.center().y);
        let end = egui::pos2(rect.left() + rect.width() * 0.75, rect.center().y);
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        let pressed = frame(
            &ctx,
            960.0,
            &mut position,
            vec![egui::Event::PointerMoved(start), button(start, true)],
            true,
        );
        assert!(pressed.drag_started());
        assert!((position - 150.0).abs() < 12.0);
        let dragged = frame(
            &ctx,
            960.0,
            &mut position,
            vec![egui::Event::PointerMoved(end)],
            true,
        );
        assert!(dragged.dragged());
        assert!((position - 450.0).abs() < 12.0);
        let released = frame(&ctx, 960.0, &mut position, vec![button(end, false)], true);
        assert!(released.drag_stopped());
    }

    #[test]
    fn disabled_timeline_does_not_seek() {
        let ctx = egui::Context::default();
        let mut position = 120.0;
        let rect = frame(&ctx, 640.0, &mut position, Vec::new(), false).rect;
        let pos = egui::pos2(rect.left() + rect.width() * 0.75, rect.center().y);
        let response = frame(
            &ctx,
            640.0,
            &mut position,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            false,
        );
        assert!(!response.changed());
        assert_eq!(position, 120.0);
    }

    #[test]
    fn navigation_buttons_stay_outside_native_video_at_every_scale() {
        for width in [640.0, 960.0, 1440.0] {
            let content =
                Rect::from_min_size(egui::pos2(16.0, 56.0), egui::vec2(width - 32.0, 300.0));
            let (video, buttons) = navigation_layout(content, true);
            assert!(video.width() > 0.0 && video.height() > 0.0);
            for button in buttons {
                assert!(content.contains_rect(button));
                assert!(button.width() >= 36.0 && button.height() >= 44.0);
                assert!(!button.intersects(video));
            }
            for scale in [1.0, 1.25, 1.5, 2.0] {
                assert!((buttons[0].right() * scale).round() <= (video.left() * scale).round());
                assert!((buttons[1].left() * scale).round() >= (video.right() * scale).round());
            }
        }
    }

    #[test]
    fn hidden_navigation_gives_the_whole_area_back_to_video() {
        let content = Rect::from_min_size(Pos2::ZERO, egui::vec2(1920.0, 1080.0));
        let (video, buttons) = navigation_layout(content, false);
        assert_eq!(video, content);
        assert!(buttons.iter().all(|button| !button.is_positive()));
    }

    #[test]
    fn file_navigation_buttons_click_only_when_enabled() {
        for previous in [true, false] {
            for enabled in [true, false] {
                let ctx = egui::Context::default();
                let rect = Rect::from_min_size(egui::pos2(20.0, 100.0), egui::vec2(40.0, 48.0));
                let pos = rect.center();
                let mut clicked = false;
                let mut draw = |events| {
                    ctx.run_ui(
                        egui::RawInput {
                            screen_rect: Some(Rect::from_min_size(
                                Pos2::ZERO,
                                egui::vec2(640.0, 420.0),
                            )),
                            events,
                            ..Default::default()
                        },
                        |ui| {
                            egui::CentralPanel::default().show(ui, |ui| {
                                clicked =
                                    file_button(ui, rect, previous, enabled, "Dosya değiştir")
                                        .clicked();
                            });
                        },
                    )
                    .drop_without_applying_deltas();
                    clicked
                };
                draw(Vec::new());
                assert!(!draw(vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::NONE,
                    }
                ]));
                assert_eq!(
                    draw(vec![egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        modifiers: egui::Modifiers::NONE,
                    }]),
                    enabled
                );
            }
        }
    }
}
