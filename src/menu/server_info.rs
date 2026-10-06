use super::char_selection::CharSelectionState;
#[cfg(not(target_os = "trueos"))]
use super::dummy_scene::Scene;
#[cfg(not(target_os = "trueos"))]
use crate::ui::ice::load_font;
use crate::{
    Direction, GlobalState, PlayState, PlayStateResult,
    menu::main::get_client_msg_error,
    render::{Drawer, GlobalsBindGroup},
    settings::Settings,
    ui::{
        Graphic,
        fonts::IcedFonts as Fonts,
        ice::{Element, IcedUi as Ui, component::neat_button, style, widget},
        img_ids::ImageGraphic,
    },
    window::{self, Event},
};
use crate::client::ServerInfo;
use common::{
    assets::{self, AssetExt},
    comp,
};
use common_base::span;
use common_net::msg::server::ServerDescription;
use i18n::LocalizationHandle;
use iced::{
    Align, Column, Container, HorizontalAlignment, Length, Row, Scrollable, VerticalAlignment,
    button, scrollable,
};
use std::collections::hash_map::DefaultHasher;
use core::{hash::Hash, hash::Hasher};
use tracing::error;

image_ids_ice! {
    struct Imgs {
        <ImageGraphic>
        button: "voxygen.element.ui.generic.buttons.button",
        button_hover: "voxygen.element.ui.generic.buttons.button_hover",
        button_press: "voxygen.element.ui.generic.buttons.button_press",
    }
}

pub struct Controls {
    fonts: Fonts,
    imgs: Imgs,
    i18n: LocalizationHandle,
    bg_img: widget::image::Handle,

    accept_button: button::State,
    decline_button: button::State,
    scrollable: scrollable::State,
    server_info: ServerInfo,
    server_description: ServerDescription,
    changed: bool,
}

pub struct ServerInfoState {
    ui: Ui,
    #[cfg(not(target_os = "trueos"))]
    scene: Scene,
    #[cfg(target_os = "trueos")]
    first_plan_queued: bool,
    #[cfg(target_os = "trueos")]
    failure_recorded: bool,
    controls: Controls,
    char_select: Option<CharSelectionState>,
}

#[derive(Clone)]
pub enum Message {
    Accept,
    Decline,
}

fn rules_hash(rules: &Option<String>) -> u64 {
    let mut hasher = DefaultHasher::default();
    rules.hash(&mut hasher);
    hasher.finish()
}

impl ServerInfoState {
    /// Create a new `MainMenuState`.
    #[expect(clippy::result_large_err)]
    pub fn try_from_server_info(
        global_state: &mut GlobalState,
        bg_img_spec: &'static str,
        char_select: CharSelectionState,
        server_info: ServerInfo,
        server_description: ServerDescription,
        force_show: bool,
    ) -> Result<Self, CharSelectionState> {
        let server = global_state.profile.servers.get(&server_info.name);

        // If there are no rules, or we've already accepted these rules, we don't need
        // this state
        if (server_description.rules.is_none()
            || server
                .is_some_and(|s| s.accepted_rules == Some(rules_hash(&server_description.rules))))
            && !force_show
        {
            #[cfg(target_os = "trueos")]
            let _ = trueos::logl::log_record(
                trueos::logl::level::IMPORTANT, "apps::voxygen",
                format_args!("Voxygen server rules: skipped rules_present={} force_show={force_show}", server_description.rules.is_some()),
            );
            return Err(char_select);
        }

        #[cfg(target_os = "trueos")]
        let _ = trueos::logl::log_record(
            trueos::logl::level::IMPORTANT, "apps::voxygen",
            format_args!("Voxygen server rules: constructing native rules screen rules_present={} force_show={force_show}", server_description.rules.is_some()),
        );

        // Load language
        let i18n = &global_state.i18n.read();
        // TODO: don't add default font twice
        #[cfg(not(target_os = "trueos"))]
        let font = load_font(&i18n.fonts().get("cyri").unwrap().asset_key);

        #[cfg(target_os = "trueos")]
        let mut ui = Ui::new_native(global_state.window.physical_size(), global_state.window.scale_factor());
        #[cfg(not(target_os = "trueos"))]
        let mut ui = Ui::new(
            &mut global_state.window,
            font,
            global_state.settings.interface.ui_scale,
        )
        .unwrap();

        let changed = server.is_some_and(|s| {
            s.accepted_rules
                .is_some_and(|accepted| accepted != rules_hash(&server_description.rules))
        });

        let state = Self {
            #[cfg(not(target_os = "trueos"))]
            scene: Scene::new(global_state.window.renderer_mut()),
            controls: Controls {
                bg_img: ui.add_graphic(Graphic::Image(
                    assets::Image::load_expect(bg_img_spec).read().to_image(),
                    None,
                )),
                imgs: Imgs::load(&mut ui).expect("Failed to load images"),
                fonts: Fonts::load(i18n.fonts(), &mut ui).expect("Impossible to load fonts"),
                i18n: global_state.i18n,
                accept_button: Default::default(),
                decline_button: Default::default(),
                scrollable: Default::default(),
                server_info,
                server_description,
                changed,
            },
            ui,
            char_select: Some(char_select),
            #[cfg(target_os = "trueos")]
            first_plan_queued: false,
            #[cfg(target_os = "trueos")]
            failure_recorded: false,
        };
        #[cfg(target_os = "trueos")]
        let _ = trueos::logl::log_record(
            trueos::logl::level::IMPORTANT, "apps::voxygen",
            format_args!("Voxygen server rules: native rules screen constructed"),
        );
        Ok(state)
    }

    fn handle_event(&mut self, event: window::Event) -> bool {
        match event {
            // Pass events to ui.
            window::Event::IcedUi(event) => {
                self.ui.handle_event(event);
                true
            },
            window::Event::ScaleFactorChanged(s) => {
                self.ui.scale_factor_changed(s);
                false
            },
            _ => false,
        }
    }
}

impl PlayState for ServerInfoState {
    fn enter(&mut self, _global_state: &mut GlobalState, _: Direction) {
        #[cfg(target_os = "trueos")]
        {
            let _ = trueos::logl::log_record(
                trueos::logl::level::IMPORTANT, "apps::voxygen",
                format_args!("Voxygen server rules: entering native rules screen"),
            );
            self.first_plan_queued = false;
            self.failure_recorded = false;
            _global_state.window.resume_menu();
            self.ui.invalidate_native();
        }
        /*
        // Updated localization in case the selected language was changed
        self.main_menu_ui
            .update_language(global_state.i18n, &global_state.settings);
        // Set scale mode in case it was change
        self.main_menu_ui
            .set_scale_mode(global_state.settings.interface.ui_scale);
        */
    }

    fn tick(&mut self, global_state: &mut GlobalState, events: Vec<Event>) -> PlayStateResult {
        span!(_guard, "tick", "<ServerInfoState as PlayState>::tick");

        // Handle window events.
        for event in events {
            // Pass all events to the ui first.
            if self.handle_event(event.clone()) {
                continue;
            }

            // Shutdown on Close, ignore all other events.
            if matches!(event, Event::Close) {
                return PlayStateResult::Shutdown;
            }
        }

        if let Some(char_select) = &mut self.char_select
            && let Err(err) = char_select.client().borrow_mut().tick(
                comp::ControllerInputs::default(),
                global_state.clock.game_dt(),
            )
        {
            error!(?err, "[server_info] Failed to tick the client");
            global_state.info_message =
                Some(get_client_msg_error(err, None, &global_state.i18n.read()));
            return PlayStateResult::Pop;
        }

        // Maintain the UI.
        let view = self.controls.view();
        #[cfg(target_os = "trueos")]
        let messages = {
            let size = global_state.window.physical_size();
            match self.ui.maintain_native(view, size, &mut global_state.clipboard) {
                Ok((messages, plan)) => {
                    if let Some(plan) = plan {
                        match global_state.window.present_menu(size, plan) {
                            Ok(()) => {
                                if !self.first_plan_queued {
                                    self.first_plan_queued = true;
                                    let _ = trueos::logl::log_record(
                                        trueos::logl::level::IMPORTANT, "apps::voxygen",
                                        format_args!("Voxygen server rules: first native plan queued extent={}x{}", size.x, size.y),
                                    );
                                }
                            }
                            Err(error) => {
                                tracing::error!(%error, "Server rules presentation failed");
                                if !self.failure_recorded {
                                    self.failure_recorded = true;
                                    let _ = trueos::logl::log_record(
                                        trueos::logl::level::ERROR, "apps::voxygen",
                                        format_args!("Voxygen server rules: native presentation failed: {error}"),
                                    );
                                }
                            }
                        }
                    }
                    messages
                }
                Err(error) => {
                    tracing::error!(%error, "Server rules layout failed");
                    if !self.failure_recorded {
                        self.failure_recorded = true;
                        let _ = trueos::logl::log_record(
                            trueos::logl::level::ERROR, "apps::voxygen",
                            format_args!("Voxygen server rules: native layout failed: {error}"),
                        );
                    }
                    Vec::new()
                }
            }
        };
        #[cfg(not(target_os = "trueos"))]
        let (messages, _) = self.ui.maintain(
            view,
            global_state.window.renderer_mut(),
            None,
            &mut global_state.clipboard,
        );

        #[expect(clippy::never_loop)] // TODO: Remove when more message types are added
        for message in messages {
            match message {
                Message::Accept => {
                    // Update last-accepted rules hash so we don't see the message again
                    if let Some(server) = global_state
                        .profile
                        .servers
                        .get_mut(&self.controls.server_info.name)
                    {
                        server.accepted_rules =
                            Some(rules_hash(&self.controls.server_description.rules));
                    }

                    return PlayStateResult::Switch(Box::new(self.char_select.take().unwrap()));
                },
                Message::Decline => return PlayStateResult::Pop,
            }
        }

        PlayStateResult::Continue
    }

    fn name(&self) -> &'static str { "Server Info" }

    fn capped_fps(&self) -> bool { true }

    fn uses_native_ui(&self) -> bool { cfg!(target_os = "trueos") }

    fn globals_bind_group(&self) -> &GlobalsBindGroup {
        #[cfg(not(target_os = "trueos"))]
        { self.scene.global_bind_group() }
        #[cfg(target_os = "trueos")]
        { panic!("Native server rules do not record scene frames") }
    }

    fn render(&self, drawer: &mut Drawer<'_>, _: &Settings) {
        #[cfg(target_os = "trueos")]
        let _ = drawer;
        #[cfg(not(target_os = "trueos"))]
        {
            // Draw the UI to the screen.
            let mut third_pass = drawer.third_pass();
            if let Some(mut ui_drawer) = third_pass.draw_ui() {
                self.ui.render(&mut ui_drawer);
            };
        }
    }
}

impl Controls {
    fn view(&mut self) -> Element<'_, Message> {
        pub const TEXT_COLOR: iced::Color = iced::Color::from_rgb(1.0, 1.0, 1.0);
        pub const IMPORTANT_TEXT_COLOR: iced::Color = iced::Color::from_rgb(1.0, 0.85, 0.5);
        pub const DISABLED_TEXT_COLOR: iced::Color = iced::Color::from_rgba(1.0, 1.0, 1.0, 0.2);

        pub const FILL_FRAC_ONE: f32 = 0.67;

        let i18n = self.i18n.read();

        // TODO: consider setting this as the default in the renderer
        let button_style = style::button::Style::new(self.imgs.button)
            .hover_image(self.imgs.button_hover)
            .press_image(self.imgs.button_press)
            .text_color(TEXT_COLOR)
            .disabled_text_color(DISABLED_TEXT_COLOR);

        let accept_button = Container::new(
            Container::new(neat_button(
                &mut self.accept_button,
                i18n.get_msg("common-accept"),
                FILL_FRAC_ONE,
                button_style,
                Some(Message::Accept),
            ))
            .max_width(200),
        )
        .width(Length::Fill)
        .align_x(Align::Center);

        let decline_button = Container::new(
            Container::new(neat_button(
                &mut self.decline_button,
                i18n.get_msg("common-decline"),
                FILL_FRAC_ONE,
                button_style,
                Some(Message::Decline),
            ))
            .max_width(200),
        )
        .width(Length::Fill)
        .align_x(Align::Center);

        let mut elements = Vec::new();

        elements.push(
            Container::new(
                iced::Text::new(i18n.get_msg("main-server-rules"))
                    .size(self.fonts.cyri.scale(36))
                    .horizontal_alignment(HorizontalAlignment::Center),
            )
            .width(Length::Fill)
            .into(),
        );

        if self.changed {
            elements.push(
                Container::new(
                    iced::Text::new(i18n.get_msg("main-server-rules-seen-before"))
                        .size(self.fonts.cyri.scale(30))
                        .color(IMPORTANT_TEXT_COLOR)
                        .horizontal_alignment(HorizontalAlignment::Center),
                )
                .width(Length::Fill)
                .into(),
            );
        }

        // elements.push(iced::Text::new(format!("{}: {}", self.server_info.name,
        // self.server_info.description))     .size(self.fonts.cyri.scale(20))
        //     .width(Length::Shrink)
        //     .horizontal_alignment(HorizontalAlignment::Center)
        //     .into());

        elements.push(
            Scrollable::new(&mut self.scrollable)
                .push(
                    iced::Text::new(
                        self.server_description
                            .rules
                            .as_deref()
                            .unwrap_or("<rules>"),
                    )
                    .size(self.fonts.cyri.scale(26))
                    .width(Length::Shrink)
                    .horizontal_alignment(HorizontalAlignment::Left)
                    .vertical_alignment(VerticalAlignment::Top),
                )
                .height(Length::Fill)
                .width(Length::Fill)
                .into(),
        );

        elements.push(
            Row::with_children(vec![decline_button.into(), accept_button.into()])
                .width(Length::Shrink)
                .height(Length::Shrink)
                .padding(25)
                .into(),
        );

        Container::new(
            Container::new(
                Column::with_children(elements)
                    .spacing(10)
                    .padding(20),
            )
            .style(
                style::container::Style::color_with_double_cornerless_border(
                    (22, 18, 16, 255).into(),
                    (11, 11, 11, 255).into(),
                    (54, 46, 38, 255).into(),
                ),
            )
                .max_width(1000)
                .align_x(Align::Center)
                // .width(Length::Shrink)
                // .height(Length::Shrink)
                .padding(15),
        )
        .style(style::container::Style::image(self.bg_img))
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Align::Center)
        .padding(50)
        .into()
    }
}
