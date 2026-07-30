use {
    crate::{
        config::{BitrateAppletConfig, Unit},
        fl, network,
    },
    cosmic::{
        self, Element,
        applet::{Size, cosmic_panel_config::PanelSize, padded_control},
        config::{CosmicTk, FontConfig},
        cosmic_config::{self, Config, CosmicConfigEntry},
        cosmic_theme::Spacing,
        iced::{
            self, Alignment, Limits, Rectangle, Subscription,
            advanced::graphics::text::cosmic_text::{
                self, Attrs, Buffer, FontSystem, Metrics, Shaping,
            },
            platform_specific::shell::wayland::commands::popup::{destroy_popup, get_popup},
            widget::{Row, column, row},
            window,
        },
        surface, theme,
        widget::{
            self, autosize, button, container, dropdown,
            rectangle_tracker::{
                RectangleTracker, RectangleUpdate, rectangle_tracker_subscription,
            },
            segmented_button, segmented_control, spin_button, toggler,
        },
    },
    std::sync::LazyLock,
    tokio,
};

static AUTOSIZE_MAIN_ID: LazyLock<widget::Id> = LazyLock::new(|| widget::Id::new("autosize-main"));
static AUTOSIZE_ICON_BTN_ID: LazyLock<widget::Id> =
    LazyLock::new(|| widget::Id::new("autosize-icon-btn"));

pub struct AppModel {
    /// Application state which is managed by the COSMIC runtime
    core: cosmic::Core,
    /// The popup id
    popup: Option<window::Id>,
    /// Configuration helper
    config_helper: Config,
    /// Configuration data that persists between application runs
    config: BitrateAppletConfig,
    /// Default network interface
    network_interfaces: Vec<String>,
    /// Selected network interface
    selected_network_interface: Option<usize>,
    /// Received bytes
    received_bytes: u64,
    /// Sent bytes
    sent_bytes: u64,
    /// Download speed
    download_speed: u64,
    download_speed_display: String,
    download_unit: String,
    /// Upload speed
    upload_speed: u64,
    upload_speed_display: String,
    upload_unit: String,
    /// Unit model
    unit_model: segmented_button::SingleSelectModel,
    /// Bits Entity
    bits_entity: segmented_button::Entity,
    /// Bytes Entity
    bytes_entity: segmented_button::Entity,
    rectangle_tracker: Option<RectangleTracker<u32>>,
    rectangle: Rectangle,
    font_system: FontSystem,
    unit_width: f32,
    data_width: f32,
    line_height: f32,
}

/// Messages emitted by the application and its widgets.
#[derive(Debug, Clone)]
pub enum Message {
    TogglePopup,
    PopupClosed(window::Id),
    UpdateConfig(BitrateAppletConfig),
    UpdateBandwidth,
    UpdateNetworkInterfaces,
    UpdateSelectedNetworkInterface(usize),
    UnitChanged(segmented_button::Entity),
    UpdateRateChanged(u8),
    ShowDownloadSpeedChanged(bool),
    ShowUploadSpeedChanged(bool),
    Rectangle(RectangleUpdate<u32>),
    ThemeChanged(cosmic::config::CosmicTk),
    Surface(surface::Action),
}

impl AppModel {
    fn select_default_network_interface(&mut self) {
        self.selected_network_interface = None;
        self.received_bytes = 0;
        self.sent_bytes = 0;
        if let Some(interface) = self.network_interfaces.get(0) {
            self.selected_network_interface = Some(0);
            self.received_bytes = network::get_received_bytes(interface).unwrap_or(0);
            self.sent_bytes = network::get_sent_bytes(interface).unwrap_or(0);
        }
    }

    fn format_speed(&self, val: f64) -> String {
        let formatted = if val >= 1000.0 {
            format!("{:.0}", val)
        } else if val >= 100.0 {
            format!("{:.1}", val)
        } else {
            format!("{:.2}", val)
        };

        // Clean up trailing zeros
        let result = formatted
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string();

        // Final truncation to ensure 5 chars max total
        result.chars().take(5).collect()
    }

    fn get_panel_size(&self) -> u32 {
        match &self.core.applet.size {
            Size::Hardcoded(_) => 16,
            Size::PanelSize(panel_size) => match panel_size {
                PanelSize::XS => 16,
                PanelSize::S => 20,
                PanelSize::M => 28,
                PanelSize::L => 32,
                PanelSize::XL => 48,
                PanelSize::Custom(s) => (*s).max(16) / 2,
            },
        }
    }

    fn get_text_width_and_height(&mut self, text: &str, font_config: &FontConfig) -> (f32, f32) {
        let panel_size = self.get_panel_size();
        let (font_size, height) = if panel_size <= 20 {
            (14.0, 20.0)
        } else if panel_size <= 28 {
            (20.0, 28.0)
        } else if panel_size <= 32 {
            (24.0, 32.0)
        } else {
            (28.0, 36.0)
        };
        let font = iced::Font::from(font_config.clone());
        let family = match font.family {
            iced::font::Family::Monospace => cosmic_text::Family::Monospace,
            iced::font::Family::Serif => cosmic_text::Family::Serif,
            iced::font::Family::SansSerif => cosmic_text::Family::SansSerif,
            iced::font::Family::Name(name) => cosmic_text::Family::Name(name),
            iced::font::Family::Cursive => cosmic_text::Family::Cursive,
            iced::font::Family::Fantasy => cosmic_text::Family::Fantasy,
        };
        let weight = match font.weight {
            iced::font::Weight::Thin => cosmic_text::Weight::THIN,
            iced::font::Weight::ExtraLight => cosmic_text::Weight::EXTRA_LIGHT,
            iced::font::Weight::Light => cosmic_text::Weight::LIGHT,
            iced::font::Weight::Normal => cosmic_text::Weight::NORMAL,
            iced::font::Weight::Medium => cosmic_text::Weight::MEDIUM,
            iced::font::Weight::Bold => cosmic_text::Weight::BOLD,
            iced::font::Weight::ExtraBold => cosmic_text::Weight::EXTRA_BOLD,
            iced::font::Weight::Black => cosmic_text::Weight::BLACK,
            iced::font::Weight::Semibold => cosmic_text::Weight::SEMIBOLD,
        };

        let style = match font.style {
            iced::font::Style::Normal => cosmic_text::Style::Normal,
            iced::font::Style::Italic => cosmic_text::Style::Italic,
            iced::font::Style::Oblique => cosmic_text::Style::Oblique,
        };
        let attrs = Attrs::new().family(family).weight(weight).style(style);

        let metrics = Metrics::new(font_size, height);
        // Create a buffer to shape the text
        let mut buffer = Buffer::new(&mut self.font_system, metrics);
        buffer.set_text(text, &attrs, Shaping::Advanced, None);
        // Force the shaping and layout engine to run
        buffer.shape_until_scroll(&mut self.font_system, true);

        let run = buffer.layout_runs().next().unwrap();
        (run.line_w, run.line_height + run.line_top)
    }

    fn set_download_speed_display(&mut self) {
        // Closest power of 2
        let download_power = if self.download_speed > 0 {
            self.download_speed.ilog2()
        } else {
            0
        };
        // Dividing by closest power of 1024
        let download_speed_rebase =
            self.download_speed as f64 / 2u64.pow(download_power - download_power % 10) as f64;
        let download_speed_display = if download_power >= 10 {
            self.format_speed(download_speed_rebase)
        } else {
            // No decimal places if speed <= 1024 bits or Bytes
            format!("{:.0}", download_speed_rebase)
        };
        let mut download_unit = String::new();
        if download_power >= 20 {
            download_unit.push_str(fl!("mega-short").as_str());
        } else if download_power >= 10 {
            download_unit.push_str(fl!("kilo-short").as_str());
        }
        match self.config.unit {
            Unit::Bits => {
                download_unit
                    .push_str(format!("{}/{}", fl!("bits-short"), fl!("second-short")).as_str());
            }
            Unit::Bytes => {
                download_unit
                    .push_str(format!("{}/{}", fl!("bytes-short"), fl!("second-short")).as_str());
            }
        }
        download_unit.push_str("  ↓");
        self.download_speed_display = download_speed_display;
        self.download_unit = download_unit;
    }

    fn set_upload_speed_display(&mut self) {
        let upload_power = if self.upload_speed > 0 {
            // Closest power of 2
            self.upload_speed.ilog2()
        } else {
            0
        };
        // Dividing by closest power of 1024
        let upload_speed_rebase =
            self.upload_speed as f64 / 2u64.pow(upload_power - upload_power % 10) as f64;
        let upload_speed_display = if upload_power >= 10 {
            self.format_speed(upload_speed_rebase)
        } else {
            // No decimal places if speed <= 1024 bits or Bytes
            format!("{:.0}", upload_speed_rebase)
        };
        let mut upload_unit = String::new();
        if upload_power >= 20 {
            upload_unit.push_str(fl!("mega-short").as_str());
        } else if upload_power >= 10 {
            upload_unit.push_str(fl!("kilo-short").as_str());
        }
        match self.config.unit {
            Unit::Bits => {
                upload_unit
                    .push_str(format!("{}/{}", fl!("bits-short"), fl!("second-short")).as_str());
            }
            Unit::Bytes => {
                upload_unit
                    .push_str(format!("{}/{}", fl!("bytes-short"), fl!("second-short")).as_str());
            }
        }
        upload_unit.push_str("  ↑");
        self.upload_speed_display = upload_speed_display;
        self.upload_unit = upload_unit;
    }

    fn horizontal_layout(&self) -> Element<'_, Message> {
        let theme = cosmic::theme::active();
        let cosmic = theme.cosmic();
        let mut elements: Vec<Element<Message>> = Vec::new();
        let mut widget_width = 0.0;
        let row_width = self.data_width + cosmic.space_none() as f32 + self.unit_width;

        if self.config.show_download_speed {
            elements.push(
                container(
                    row!(
                        container(self.core.applet.text(&self.download_speed_display))
                            .align_left(self.data_width),
                        container(self.core.applet.text(&self.download_unit))
                            .align_right(self.unit_width),
                    )
                    .spacing(cosmic.space_none())
                    .clip(true),
                )
                .width(row_width)
                .height(self.line_height)
                .into(),
            );
            widget_width += row_width;
        }
        if self.config.show_upload_speed {
            if self.config.show_download_speed {
                widget_width += cosmic.space_xs() as f32;
            }
            elements.push(
                container(
                    row!(
                        container(self.core.applet.text(&self.upload_speed_display))
                            .align_left(self.data_width),
                        container(self.core.applet.text(&self.upload_unit))
                            .align_right(self.unit_width),
                    )
                    .spacing(cosmic.space_none())
                    .clip(true),
                )
                .width(row_width)
                .height(self.line_height)
                .into(),
            );
            widget_width += row_width;
        }

        let padding = self.core.applet.suggested_padding(true);
        widget_width += 2.0 * padding.0 as f32;
        container(
            Row::from_vec(elements)
                .spacing(cosmic.space_xs())
                .clip(true),
        )
        .align_y(Alignment::Center)
        .padding([padding.1, padding.0])
        .height(self.line_height + 2.0 * padding.1 as f32)
        .width(widget_width)
        .into()
    }
}

impl cosmic::Application for AppModel {
    type Executor = cosmic::executor::Default;

    type Flags = ();

    type Message = Message;

    const APP_ID: &'static str = "io.github.avomar.cosmic-ext-applet-bitrate";

    fn core(&self) -> &cosmic::Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut cosmic::Core {
        &mut self.core
    }

    fn init(
        core: cosmic::Core,
        _flags: Self::Flags,
    ) -> (Self, cosmic::Task<cosmic::Action<Self::Message>>) {
        let config_helper =
            cosmic_config::Config::new(Self::APP_ID, BitrateAppletConfig::VERSION).unwrap();
        let config = cosmic_config::Config::new(Self::APP_ID, BitrateAppletConfig::VERSION)
            .map(|context| match BitrateAppletConfig::get_entry(&context) {
                Ok(config) => config,
                Err((_errors, config)) => config,
            })
            .unwrap_or_default();

        let mut bits_entity = segmented_button::Entity::default();
        let mut bytes_entity = segmented_button::Entity::default();
        let mut unit_model = segmented_button::SingleSelectModel::builder()
            .insert(|b| b.text(fl!("bits")).with_id(|id| bits_entity = id))
            .insert(|b| b.text(fl!("bytes")).with_id(|id| bytes_entity = id))
            .build();

        if config.unit == Unit::Bits {
            unit_model.activate(bits_entity);
        } else if config.unit == Unit::Bytes {
            unit_model.activate(bytes_entity);
        }

        // Set initial received and sent bytes
        let network_interfaces = network::get_network_interfaces();
        let mut selected_network_interface: Option<usize> = None;
        let mut received_bytes = 0;
        let mut sent_bytes = 0;
        if let Some(interface) = network_interfaces.get(0) {
            selected_network_interface = Some(0);
            received_bytes = network::get_received_bytes(interface).unwrap_or(0);
            sent_bytes = network::get_sent_bytes(interface).unwrap_or(0);
        }

        // Construct the app model with the runtime's core.
        let mut app = AppModel {
            core,
            config_helper,
            config,
            popup: None,
            received_bytes,
            sent_bytes,
            download_speed: 0,
            download_speed_display: "".to_string(),
            download_unit: "".to_string(),
            upload_speed: 0,
            upload_speed_display: "".to_string(),
            upload_unit: "".to_string(),
            network_interfaces: network_interfaces,
            selected_network_interface,
            unit_model,
            bits_entity,
            bytes_entity,
            rectangle: Rectangle::default(),
            rectangle_tracker: None,
            font_system: FontSystem::new(),
            unit_width: 0.0,
            data_width: 0.0,
            line_height: 0.0,
        };
        app.set_download_speed_display();
        app.set_upload_speed_display();
        let interface_font = match CosmicTk::get_entry(
            &Config::new("com.system76.CosmicTk", CosmicTk::VERSION).unwrap(),
        ) {
            Ok(cosmic_tk) => cosmic_tk.interface_font,
            Err((_, cosmic_tk)) => cosmic_tk.interface_font,
        };
        app.data_width = app.get_text_width_and_height("00.00", &interface_font).0;
        app.unit_width = app.get_text_width_and_height("MB/s  ↓", &interface_font).0;
        app.line_height = app
            .get_text_width_and_height("1234567890.KM/Bb↓↑", &interface_font)
            .1;
        (app, cosmic::Task::none())
    }

    fn on_close_requested(&self, id: window::Id) -> Option<Message> {
        Some(Message::PopupClosed(id))
    }

    fn view(&self) -> Element<'_, Self::Message> {
        let is_horizontal = self.core.applet.is_horizontal();
        let mut limits = Limits::NONE.min_width(1.).min_height(1.);
        if let Some(b) = self.core.applet.suggested_bounds {
            if b.width > 0.0 {
                limits = limits.max_width(b.width);
            }
            if b.height > 0.0 {
                limits = limits.max_height(b.height);
            }
        }

        let button: Element<'_, Self::Message>;
        // TODO: Try with single autosize_id after iced rebase to 0.14
        let autosize_id: widget::Id;
        if is_horizontal && (self.config.show_download_speed || self.config.show_upload_speed) {
            autosize_id = AUTOSIZE_MAIN_ID.clone();
            button = button::custom(self.horizontal_layout())
                .padding(0)
                .on_press_down(Message::TogglePopup)
                .class(cosmic::theme::Button::AppletIcon)
                .into();
        } else {
            autosize_id = AUTOSIZE_ICON_BTN_ID.clone();
            button = self
                .core
                .applet
                .applet_tooltip::<Message>(
                    self.core
                        .applet
                        .icon_button(Self::APP_ID)
                        .on_press_down(Message::TogglePopup)
                        .class(cosmic::theme::Button::AppletIcon),
                    format!(
                        "{} {}  {} {}",
                        self.download_speed_display,
                        self.download_unit,
                        self.upload_speed_display,
                        self.upload_unit
                    ),
                    self.popup.is_some(),
                    Message::Surface,
                    None,
                )
                .into();
        }

        autosize::autosize(
            if let Some(tracker) = self.rectangle_tracker.as_ref() {
                tracker.container(0, button).ignore_bounds(true).into()
            } else {
                button
            },
            autosize_id,
        )
        .limits(limits)
        .into()
    }

    fn view_window(&self, _id: window::Id) -> Element<'_, Self::Message> {
        let Spacing {
            space_xxxs,
            space_xxs,
            space_s,
            ..
        } = theme::active().cosmic().spacing;
        let content = column!(
            padded_control(widget::settings::item(
                fl!("network-interface"),
                dropdown(
                    self.network_interfaces.clone(),
                    self.selected_network_interface,
                    Message::UpdateSelectedNetworkInterface
                )
            )),
            padded_control(widget::divider::horizontal::default()).padding([space_xxs, space_s]),
            padded_control(
                column!(
                    widget::text::body(fl!("unit")),
                    segmented_control::horizontal(&self.unit_model)
                        .on_activate(Message::UnitChanged)
                )
                .spacing(space_xxxs)
            ),
            padded_control(widget::divider::horizontal::default()).padding([space_xxs, space_s]),
            padded_control(widget::settings::item(
                fl!("update-rate"),
                spin_button::spin_button(
                    format!("{} {}", self.config.update_rate, fl!("second-short")),
                    self.config.update_rate,
                    1,
                    1,
                    10,
                    Message::UpdateRateChanged,
                ),
            )),
            padded_control(widget::divider::horizontal::default()).padding([space_xxs, space_s]),
            padded_control(widget::settings::item(
                fl!("show-download-speed"),
                toggler(self.config.show_download_speed)
                    .on_toggle(Message::ShowDownloadSpeedChanged)
            )),
            padded_control(widget::divider::horizontal::default()).padding([space_xxs, space_s]),
            padded_control(widget::settings::item(
                fl!("show-upload-speed"),
                toggler(self.config.show_upload_speed).on_toggle(Message::ShowUploadSpeedChanged)
            ))
        )
        .padding([8, 0]);

        self.core.applet.popup_container(content).into()
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        Subscription::batch(vec![
            rectangle_tracker_subscription(0).map(|e| Message::Rectangle(e.1)),
            (iced::time::every(tokio::time::Duration::from_secs(
                self.config.update_rate as u64,
            )))
            .map(|_| Message::UpdateBandwidth),
            (iced::time::every(tokio::time::Duration::from_secs(5)))
                .map(|_| Message::UpdateNetworkInterfaces),
            // Watch for application configuration changes.
            self.core()
                .watch_config::<BitrateAppletConfig>(Self::APP_ID)
                .map(|update| Message::UpdateConfig(update.config)),
            self.core()
                .watch_config("com.system76.CosmicTk")
                .map(|u| Message::ThemeChanged(u.config)),
        ])
    }

    fn update(&mut self, message: Self::Message) -> cosmic::Task<cosmic::Action<Self::Message>> {
        match message {
            Message::UpdateBandwidth => {
                if let Some(selected_network_interface) = self.selected_network_interface {
                    let network_interface =
                        self.network_interfaces[selected_network_interface].clone();
                    if let Some(received_bytes_cur) =
                        network::get_received_bytes(network_interface.as_ref())
                    {
                        self.download_speed = received_bytes_cur - self.received_bytes;
                        if self.config.unit == Unit::Bits {
                            self.download_speed *= 8;
                        }
                        self.download_speed /= self.config.update_rate as u64;
                        self.received_bytes = received_bytes_cur;
                        self.set_download_speed_display();
                    }
                    if let Some(sent_bytes_cur) =
                        network::get_sent_bytes(network_interface.as_ref())
                    {
                        self.upload_speed = sent_bytes_cur - self.sent_bytes;
                        if self.config.unit == Unit::Bits {
                            self.upload_speed *= 8;
                        }
                        self.upload_speed /= self.config.update_rate as u64;
                        self.sent_bytes = sent_bytes_cur;
                        self.set_upload_speed_display();
                    }
                } else {
                    self.download_speed = 0;
                    self.upload_speed = 0;
                }
            }
            Message::UpdateNetworkInterfaces => {
                if let Some(selected_interface) = self.selected_network_interface {
                    let selected_network_interface = self
                        .network_interfaces
                        .get(selected_interface)
                        .unwrap()
                        .clone();
                    self.network_interfaces = network::get_network_interfaces();
                    if let Some(selected_interface_index) = self
                        .network_interfaces
                        .iter()
                        .position(|interface| &selected_network_interface == interface)
                    {
                        self.selected_network_interface = Some(selected_interface_index);
                    } else {
                        self.select_default_network_interface();
                    }
                } else {
                    self.network_interfaces = network::get_network_interfaces();
                    self.select_default_network_interface();
                }
            }
            Message::UpdateSelectedNetworkInterface(new_interface) => {
                self.selected_network_interface = Some(new_interface);
                let interface = self.network_interfaces.get(0).unwrap();
                self.received_bytes = network::get_received_bytes(interface).unwrap_or(0);
                self.sent_bytes = network::get_sent_bytes(interface).unwrap_or(0);
            }
            Message::UnitChanged(entity) => {
                if !self.unit_model.is_active(entity) {
                    self.unit_model.activate(entity);
                    if entity == self.bits_entity {
                        self.download_speed *= 8;
                        self.upload_speed *= 8;
                        self.config
                            .set_unit(&self.config_helper, Unit::Bits)
                            .unwrap();
                    } else if entity == self.bytes_entity {
                        self.download_speed /= 8;
                        self.upload_speed /= 8;
                        self.config
                            .set_unit(&self.config_helper, Unit::Bytes)
                            .unwrap();
                    }
                    self.set_download_speed_display();
                    self.set_upload_speed_display();
                }
            }
            Message::UpdateRateChanged(rate) => {
                self.config
                    .set_update_rate(&self.config_helper, rate)
                    .unwrap();
            }
            Message::ShowDownloadSpeedChanged(show) => {
                self.config
                    .set_show_download_speed(&self.config_helper, show)
                    .unwrap();
            }
            Message::ShowUploadSpeedChanged(show) => {
                self.config
                    .set_show_upload_speed(&self.config_helper, show)
                    .unwrap();
            }
            Message::Rectangle(u) => match u {
                RectangleUpdate::Rectangle(r) => {
                    self.rectangle = r.1;
                }
                RectangleUpdate::Init(tracker) => {
                    self.rectangle_tracker = Some(tracker);
                }
            },
            Message::UpdateConfig(config) => {
                self.config = config;
            }
            Message::TogglePopup => {
                return if let Some(p) = self.popup.take() {
                    destroy_popup(p)
                } else {
                    let new_id = window::Id::unique();
                    self.popup.replace(new_id);
                    let mut popup_settings = self.core.applet.get_popup_settings(
                        self.core().main_window_id().unwrap(),
                        new_id,
                        None,
                        None,
                        None,
                    );
                    let Rectangle {
                        x,
                        y,
                        width,
                        height,
                    } = self.rectangle;
                    popup_settings.positioner.anchor_rect = Rectangle::<i32> {
                        x: x.max(1.) as i32,
                        y: y.max(1.) as i32,
                        width: width.max(1.) as i32,
                        height: height.max(1.) as i32,
                    };
                    get_popup(popup_settings)
                };
            }
            Message::ThemeChanged(theme) => {
                self.data_width = self
                    .get_text_width_and_height("00.00", &theme.interface_font)
                    .0;
                self.unit_width = self
                    .get_text_width_and_height("MB/s  ↓", &theme.interface_font)
                    .0;
                self.line_height = self
                    .get_text_width_and_height("1234567890.KM/Bb↓↑", &theme.interface_font)
                    .1;
            }
            Message::PopupClosed(id) => {
                self.popup.take_if(|stored_id| stored_id == &id);
            }
            Message::Surface(a) => {
                return cosmic::task::message(cosmic::Action::Cosmic(
                    cosmic::app::Action::Surface(a),
                ));
            }
        }
        cosmic::Task::none()
    }

    fn style(&self) -> Option<cosmic::iced::theme::Style> {
        Some(cosmic::applet::style())
    }
}
