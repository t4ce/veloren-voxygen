mod connecting;
// Note: Keeping in case we re-add the disclaimer
//mod disclaimer;
mod credits;
#[path = "IGAccCreate.rs"]
mod ig_acc_create;
mod login;
mod login_focus;
mod logo_glow;
mod quit;
mod selection_panel;
mod servers;

use crate::{
    GlobalState,
    credits::Credits,
    render::UiDrawer,
    ui::{
        self, Graphic,
        fonts::IcedFonts as Fonts,
        ice::{Element, IcedUi as Ui, load_font, style, widget},
        img_ids::ImageGraphic,
    },
    window,
};
use i18n::{LanguageMetadata, LocalizationHandle};
use iced::{Column, Container, Length, text_input};
//ImageFrame, Tooltip,
use crate::settings::Settings;
use common::assets::{AssetExt, Image, Ron};
use core::time::Duration;
use rand::{rng, seq::IndexedRandom};
use tracing::warn;

use super::DetailedInitializationStage;

// TODO: what is this? (showed up in rebase)
//const COL1: Color = Color::Rgba(0.07, 0.1, 0.1, 0.9);

pub const TEXT_COLOR: iced::Color = iced::Color::from_rgb(1.0, 1.0, 1.0);
pub const DISABLED_TEXT_COLOR: iced::Color = iced::Color::from_rgba(1.0, 1.0, 1.0, 0.2);

pub const FILL_FRAC_ONE: f32 = 0.67;
pub const FILL_FRAC_TWO: f32 = 0.53;

image_ids_ice! {
    struct Imgs {
        <ImageGraphic>
        v_logo: "voxygen.element.v_logo",
        bg: "voxygen.background.bg_main",
        banner_top: "voxygen.element.ui.generic.frames.banner_top",
        button: "voxygen.element.ui.generic.buttons.button",
        button_hover: "voxygen.element.ui.generic.buttons.button_hover",
        button_press: "voxygen.element.ui.generic.buttons.button_press",
        input_bg: "voxygen.element.ui.generic.textbox",
        loading_art: "voxygen.element.ui.generic.frames.loading_screen.loading_bg",
        loading_art_l: "voxygen.element.ui.generic.frames.loading_screen.loading_bg_l",
        loading_art_r: "voxygen.element.ui.generic.frames.loading_screen.loading_bg_r",
        selection: "voxygen.element.ui.generic.frames.selection",
        selection_hover: "voxygen.element.ui.generic.frames.selection_hover",
        selection_press: "voxygen.element.ui.generic.frames.selection_press",

        unlock: "voxygen.element.ui.generic.buttons.unlock",
        unlock_hover: "voxygen.element.ui.generic.buttons.unlock_hover",
        unlock_press: "voxygen.element.ui.generic.buttons.unlock_press",
    }
}

// Randomly loaded background images
const BG_IMGS: [&str; 41] = [
    "voxygen.background.bg_1",
    "voxygen.background.bg_2",
    "voxygen.background.bg_3",
    "voxygen.background.bg_4",
    "voxygen.background.bg_5",
    "voxygen.background.bg_6",
    "voxygen.background.bg_7",
    "voxygen.background.bg_8",
    "voxygen.background.bg_9",
    "voxygen.background.bg_10",
    "voxygen.background.bg_11",
    "voxygen.background.bg_12",
    "voxygen.background.bg_13",
    "voxygen.background.bg_14",
    "voxygen.background.bg_15",
    "voxygen.background.bg_16",
    "voxygen.background.bg_17",
    "voxygen.background.bg_18",
    "voxygen.background.bg_19",
    "voxygen.background.bg_20",
    "voxygen.background.bg_21",
    "voxygen.background.bg_22",
    "voxygen.background.bg_23",
    "voxygen.background.bg_24",
    "voxygen.background.bg_25",
    "voxygen.background.bg_26",
    "voxygen.background.bg_27",
    "voxygen.background.bg_28",
    "voxygen.background.bg_29",
    "voxygen.background.bg_30",
    "voxygen.background.bg_31",
    "voxygen.background.bg_32",
    "voxygen.background.bg_33",
    "voxygen.background.bg_34",
    "voxygen.background.bg_35",
    "voxygen.background.bg_36",
    "voxygen.background.bg_37",
    "voxygen.background.bg_38",
    "voxygen.background.bg_39",
    "voxygen.background.bg_40",
    "voxygen.background.bg_41",
];

pub enum Event {
    LoginAttempt {
        username: String,
        password: String,
        server_address: String,
    },
    CancelLoginAttempt,
    ChangeLanguage(LanguageMetadata),
    Quit,
    // Note: Keeping in case we re-add the disclaimer
    //DisclaimerAccepted,
    AuthServerTrust(String, bool),
    DeleteServer {
        server_index: usize,
    },
}

pub struct LoginInfo {
    pub username: String,
    pub password: String,
    pub server: String,
}

enum ConnectionState {
    InProgress,
    AssetSync { message: String, cancellable: bool },
    AuthTrustPrompt { auth_server: String, msg: String },
}

enum Screen {
    // Note: Keeping in case we re-add the disclaimer
    /*Disclaimer {
        screen: disclaimer::Screen,
    },*/
    Credits {
        screen: credits::Screen,
    },
    Login {
        screen: Box<login::Screen>, // boxed to avoid large variant
        // Error to display in a box
        error: Option<String>,
    },
    Servers {
        screen: servers::Screen,
    },
    Connecting {
        screen: connecting::Screen,
        connection_state: ConnectionState,
        init_stage: DetailedInitializationStage,
    },
    #[cfg(feature = "picasso-assets")]
    AssetSync {
        screen: connecting::Screen,
    },
}

#[derive(PartialEq, Eq)]
enum Showing {
    Login,
    Languages,
    Account,
}

impl Showing {
    fn toggle(&mut self, other: Showing) {
        if *self == other {
            *self = Showing::Login;
        } else {
            *self = other;
        }
    }
}

pub struct Controls {
    fonts: Fonts,
    imgs: Imgs,
    logo_glow: logo_glow::LogoGlow,
    bg_img: widget::image::Handle,
    i18n: LocalizationHandle,
    credits: Credits,

    // If a server address was provided via cli argument we hide the server list button and replace
    // the server field with a plain label (with a button to exit this mode and freely edit the
    // field).
    server_field_locked: bool,
    selected_server_index: Option<usize>,
    login_info: LoginInfo,

    show: Showing,
    selected_language_index: Option<usize>,

    time: f64,

    screen: Screen,
    loading_started: Option<std::time::Instant>,
    pending_connection_error: Option<String>,
    dialog_chrome: login::Screen,
    quit_dialog: quit::Screen,
    confirming_quit: bool,
    #[cfg(feature = "picasso-assets")]
    asset_sync: Option<crate::asset_sync::Job>,
    #[cfg(feature = "picasso-assets")]
    asset_sync_previous: Option<Box<Screen>>,
}

#[derive(Clone)]
enum Message {
    AssetSync,
    CancelAssetSync,
    Quit,
    ConfirmQuit,
    BackFromQuit,
    Back,
    ShowServers,
    ShowAccount,
    AccountField(usize, String),
    AccountFocus(usize),
    CreateAccount,
    AccountLink(&'static str),
    AccountBack,
    ShowCredits,
    Multiplayer,
    UnlockServerField,
    LanguageChanged(usize),
    OpenLanguageMenu,
    Username(String),
    Password(String),
    Server(String),
    ServerChanged(usize),
    FocusPassword,
    CancelConnect,
    TrustPromptAdd,
    TrustPromptCancel,
    CloseError,
    DeleteServer,
    /* Note: Keeping in case we re-add the disclaimer
     *AcceptDisclaimer, */
}

impl Controls {
    fn new(
        fonts: Fonts,
        imgs: Imgs,
        bg_img: widget::image::Handle,
        i18n: LocalizationHandle,
        settings: &Settings,
        server: Option<String>,
        logo_glow: logo_glow::LogoGlow,
    ) -> Self {
        let credits = Ron::<Credits>::load_expect_cloned("credits").into_inner();

        // Note: Keeping in case we re-add the disclaimer
        let screen = /* if settings.show_disclaimer {
            Screen::Disclaimer {
                screen: disclaimer::Screen::new(),
            }
        } else { */
            Screen::Login {
                screen: Box::default(),
                error: None,
            };
        //};

        let server_field_locked = server.is_some();
        let login_info = LoginInfo {
            username: settings.networking.username.clone(),
            password: String::new(),
            server: server.unwrap_or_else(|| settings.networking.default_server.clone()),
        };
        let selected_server_index = settings
            .networking
            .servers
            .iter()
            .position(|f| f == &login_info.server);

        let language_metadatas = i18n::list_localizations();
        let selected_language_index = language_metadatas
            .iter()
            .position(|f| f.language_identifier == settings.language.selected_language);

        Self {
            fonts,
            imgs,
            logo_glow,
            bg_img,
            i18n,
            credits,

            server_field_locked,
            selected_server_index,
            login_info,

            show: Showing::Login,
            selected_language_index,

            time: 0.0,

            screen,
            loading_started: None,
            pending_connection_error: None,
            dialog_chrome: login::Screen::default(),
            quit_dialog: quit::Screen::default(),
            confirming_quit: false,
            #[cfg(feature = "picasso-assets")]
            asset_sync: None,
            #[cfg(feature = "picasso-assets")]
            asset_sync_previous: None,
        }
    }

    fn view(&mut self, settings: &Settings, dt: f32) -> Element<'_, Message> {
        self.time += dt as f64;
        if self.loading_minimum_elapsed() {
            if let Some(error) = self.pending_connection_error.take() {
                self.connection_error(error);
            }
        }

        // TODO: consider setting this as the default in the renderer
        let button_style = style::button::Style::new(self.imgs.button)
            .hover_image(self.imgs.button_hover)
            .press_image(self.imgs.button_press)
            .text_color(TEXT_COLOR)
            .disabled_text_color(DISABLED_TEXT_COLOR);

        let loading = matches!(&self.screen, Screen::Connecting { .. });
        #[cfg(feature = "picasso-assets")]
        let loading = loading || matches!(&self.screen, Screen::AssetSync { .. });
        let bg_img = if loading { self.bg_img } else { self.imgs.bg };

        let language_metadatas = i18n::list_localizations();

        if self.confirming_quit {
            let content = self
                .quit_dialog
                .view(&self.fonts, &self.i18n.read(), button_style);
            let content = self.dialog_chrome.view(
                &self.fonts,
                &self.imgs,
                &self.logo_glow,
                self.server_field_locked,
                &self.login_info,
                None,
                &self.i18n.read(),
                &Showing::Login,
                self.selected_language_index,
                &language_metadatas,
                button_style,
                Some((6, content)),
            );
            return Container::new(content)
                .width(Length::Fill)
                .height(Length::Fill)
                .style(style::container::Style::image(bg_img))
                .into();
        }

        // TODO: make any large text blocks scrollable so that if the area is to
        // small they can still be read
        let content = match &mut self.screen {
            // Note: Keeping in case we re-add the disclaimer
            //Screen::Disclaimer { screen } => screen.view(&self.fonts, &self.i18n, button_style),
            Screen::Credits { screen } => {
                let content = screen.view(
                    &self.fonts,
                    &self.imgs,
                    &self.i18n.read(),
                    &self.credits,
                    button_style,
                );
                self.dialog_chrome.view(
                    &self.fonts,
                    &self.imgs,
                    &self.logo_glow,
                    self.server_field_locked,
                    &self.login_info,
                    None,
                    &self.i18n.read(),
                    &Showing::Login,
                    self.selected_language_index,
                    &language_metadatas,
                    button_style,
                    Some((3, content)),
                )
            }
            Screen::Login { screen, error } => screen.view(
                &self.fonts,
                &self.imgs,
                &self.logo_glow,
                self.server_field_locked,
                &self.login_info,
                error.as_deref(),
                &self.i18n.read(),
                &self.show,
                self.selected_language_index,
                &language_metadatas,
                button_style,
                None,
            ),
            Screen::Servers { screen } => {
                let content = screen.view(
                    &self.fonts,
                    &self.imgs,
                    &settings.networking.servers,
                    self.selected_server_index,
                    &self.i18n.read(),
                    button_style,
                );
                self.dialog_chrome.view(
                    &self.fonts,
                    &self.imgs,
                    &self.logo_glow,
                    self.server_field_locked,
                    &self.login_info,
                    None,
                    &self.i18n.read(),
                    &Showing::Login,
                    self.selected_language_index,
                    &language_metadatas,
                    button_style,
                    Some((4, content)),
                )
            }
            Screen::Connecting {
                screen,
                connection_state,
                init_stage,
            } => screen.view(
                &self.fonts,
                &self.imgs,
                connection_state,
                init_stage,
                self.time,
                &self.i18n.read(),
                button_style,
                settings.interface.loading_tips,
                &settings.controls,
            ),
            #[cfg(feature = "picasso-assets")]
            Screen::AssetSync { screen } => {
                let status = self.asset_sync.as_ref().unwrap().status();
                screen.view(
                    &self.fonts,
                    &self.imgs,
                    &ConnectionState::AssetSync {
                        message: status.message,
                        cancellable: status.cancellable,
                    },
                    &DetailedInitializationStage::StartingMultiplayer,
                    self.time,
                    &self.i18n.read(),
                    button_style,
                    false,
                    &settings.controls,
                )
            }
        };

        Container::new(
            Column::with_children(vec![content])
                .spacing(3)
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .style(style::container::Style::image(bg_img))
        .into()
    }

    fn update(
        &mut self,
        message: Message,
        events: &mut Vec<Event>,
        settings: &Settings,
        ui: &mut Ui,
        runtime: &alloc::sync::Arc<tokio::runtime::Runtime>,
    ) {
        #[cfg(feature = "picasso-assets")]
        if let Some(job) = &self.asset_sync {
            if matches!(message, Message::CancelAssetSync) {
                job.cancel();
            }
            return;
        }
        if self.confirming_quit {
            match message {
                Message::ConfirmQuit => events.push(Event::Quit),
                Message::BackFromQuit => self.confirming_quit = false,
                _ => {}
            }
            return;
        }
        if matches!(&self.screen, Screen::Connecting { .. })
            && !matches!(
                &message,
                Message::Quit
                    | Message::CancelConnect
                    | Message::TrustPromptAdd
                    | Message::TrustPromptCancel
            )
        {
            return;
        }
        let servers = &settings.networking.servers;
        let mut language_metadatas = i18n::list_localizations();

        match message {
            Message::AssetSync => {
                #[cfg(feature = "picasso-assets")]
                {
                    self.asset_sync = Some(crate::asset_sync::Job::start(
                        self.login_info.server.clone(),
                        runtime,
                    ));
                    let screen = Screen::AssetSync {
                        screen: connecting::Screen::new(ui),
                    };
                    self.asset_sync_previous =
                        Some(Box::new(std::mem::replace(&mut self.screen, screen)));
                }
                #[cfg(not(feature = "picasso-assets"))]
                self.connection_error("Asset Sync requires the Picasso asset database.".into());
            }
            Message::CancelAssetSync => {}
            Message::Quit => self.request_quit(),
            Message::ConfirmQuit | Message::BackFromQuit => {}
            Message::Back => {
                self.show = Showing::Login;
                self.screen = Screen::Login {
                    screen: Box::default(),
                    error: None,
                };
            }
            Message::ShowAccount => {
                if !matches!(self.screen, Screen::Login { .. }) {
                    self.screen = Screen::Login {
                        screen: Box::default(),
                        error: None,
                    };
                }
                self.show = Showing::Account;
                if let Screen::Login { screen, error } = &mut self.screen {
                    *error = None;
                    screen.account_created = false;
                    screen.account.focus(0);
                }
            }
            Message::AccountBack => self.show = Showing::Login,
            Message::AccountField(idx, value) => {
                if self.show == Showing::Account {
                    if let Screen::Login { screen, .. } = &mut self.screen {
                        screen.account.field(idx, value);
                    }
                }
            }
            Message::AccountFocus(idx) => {
                if let Screen::Login { screen, .. } = &mut self.screen {
                    screen.account.focus(idx);
                }
            }
            Message::CreateAccount => {
                if self.show == Showing::Account {
                    if let Screen::Login { screen, error } = &mut self.screen {
                        *error = screen.account.submit(runtime, &self.i18n.read());
                    }
                }
            }
            Message::AccountLink(path) => {
                if let Err(err) = open::that(format!("https://veloren.net/account/{path}/")) {
                    if let Screen::Login { error, .. } = &mut self.screen {
                        *error = Some(format!(
                            "{}: {err}",
                            self.i18n.read().get_msg("main-account-open_failed")
                        ));
                    }
                }
            }
            Message::ShowServers => {
                self.show = Showing::Login;
                self.selected_server_index =
                    servers.iter().position(|f| f == &self.login_info.server);
                self.screen = Screen::Servers {
                    screen: servers::Screen::new(),
                };
            }
            Message::ShowCredits => {
                self.show = Showing::Login;
                self.screen = Screen::Credits {
                    screen: credits::Screen::new(),
                };
            }
            Message::Multiplayer => {
                self.begin_connection(ui);

                events.push(self.login_attempt());
            }
            Message::UnlockServerField => self.server_field_locked = false,
            Message::Username(new_value) => self.login_info.username = new_value,
            Message::LanguageChanged(new_value) => {
                events.push(Event::ChangeLanguage(language_metadatas.remove(new_value)));
            }
            Message::OpenLanguageMenu => {
                if !matches!(self.screen, Screen::Login { .. }) {
                    self.screen = Screen::Login {
                        screen: Box::default(),
                        error: None,
                    };
                    self.show = Showing::Login;
                }
                if let Screen::Login { error, .. } = &mut self.screen {
                    *error = None;
                }
                self.show.toggle(Showing::Languages);
            }
            Message::Password(new_value) => self.login_info.password = new_value,
            Message::Server(new_value) => {
                self.login_info.server = new_value;
            }
            Message::ServerChanged(new_value) => {
                self.selected_server_index = Some(new_value);
                self.login_info.server.clone_from(&servers[new_value]);
            }
            Message::FocusPassword => {
                if let Screen::Login { screen, .. } = &mut self.screen {
                    screen.banner.multiplayer_focus.focus(false);
                    screen.banner.password = text_input::State::focused();
                    screen.banner.username = text_input::State::new();
                }
            }
            Message::CancelConnect => {
                self.exit_connect_screen();
                events.push(Event::CancelLoginAttempt);
            }
            msg @ Message::TrustPromptAdd | msg @ Message::TrustPromptCancel => {
                if let Screen::Connecting {
                    connection_state, ..
                } = &mut self.screen
                    && let ConnectionState::AuthTrustPrompt { auth_server, .. } = connection_state
                {
                    let auth_server = core::mem::take(auth_server);
                    let added = matches!(msg, Message::TrustPromptAdd);

                    *connection_state = ConnectionState::InProgress;
                    events.push(Event::AuthServerTrust(auth_server, added));
                }
            }
            Message::CloseError => {
                if let Screen::Login { screen, error } = &mut self.screen {
                    *error = None;
                    if core::mem::take(&mut screen.account_created) {
                        self.show = Showing::Login;
                        screen.banner.username = text_input::State::new();
                        screen.banner.server = text_input::State::new();
                        screen.banner.password = text_input::State::focused();
                    }
                }
            }
            Message::DeleteServer => {
                if let Some(server_index) = self.selected_server_index {
                    events.push(Event::DeleteServer { server_index });
                    self.selected_server_index = None;
                }
            } /* Note: Keeping in case we re-add the disclaimer */
              /*Message::AcceptDisclaimer => {
                  if let Screen::Disclaimer { .. } = &self.screen {
                      events.push(Event::DisclaimerAccepted);
                      self.screen = Screen::Login {
                          screen: login::Screen::default(),
                          error: None,
                      };
                  }
              },*/
        }
    }

    fn login_attempt(&self) -> Event {
        Event::LoginAttempt {
            username: self.login_info.username.trim().to_string(),
            password: self.login_info.password.clone(),
            server_address: self.login_info.server.trim().to_string(),
        }
    }

    // One minimum duration for every connecting-screen spawn. Never block the UI loop.
    const MIN_SHOW_SEC: u64 = 3;

    fn begin_connection(&mut self, ui: &mut Ui) {
        self.loading_started = Some(std::time::Instant::now());
        self.pending_connection_error = None;
        self.screen = Screen::Connecting {
            screen: connecting::Screen::new(ui),
            connection_state: ConnectionState::InProgress,
            init_stage: DetailedInitializationStage::StartingMultiplayer,
        };
    }

    fn loading_minimum_elapsed(&self) -> bool {
        self.loading_started
            .is_none_or(|started| started.elapsed() >= Duration::from_secs(Self::MIN_SHOW_SEC))
    }

    // Explicit cancellation bypasses the minimum and discards a deferred error.
    fn exit_connect_screen(&mut self) {
        self.loading_started = None;
        self.pending_connection_error = None;
        if matches!(&self.screen, Screen::Connecting { .. }) {
            self.screen = Screen::Login {
                screen: Box::default(),
                error: None,
            }
        }
    }

    fn auth_trust_prompt(&mut self, auth_server: String) {
        if let Screen::Connecting {
            connection_state, ..
        } = &mut self.screen
        {
            let msg = format!(
                "Warning: The server you are trying to connect to has provided this \
                 authentication server address:\n\n{}\n\nbut it is not in your list of trusted \
                 authentication servers.\n\nMake sure that you trust this site and owner to not \
                 try and bruteforce your password!",
                auth_server
            );

            *connection_state = ConnectionState::AuthTrustPrompt { auth_server, msg };
        }
    }

    fn connection_error(&mut self, error: String) {
        if matches!(&self.screen, Screen::Connecting { .. }) && !self.loading_minimum_elapsed() {
            self.pending_connection_error = Some(error);
            return;
        }
        self.loading_started = None;
        self.pending_connection_error = None;
        if matches!(&self.screen, Screen::Connecting { .. })
            || matches!(&self.screen, Screen::Login { .. })
        {
            self.screen = Screen::Login {
                screen: Box::default(),
                error: Some(error),
            }
        } else {
            warn!("connection_error invoked on unhandled screen!");
        }
    }

    fn update_init_stage(&mut self, stage: DetailedInitializationStage) {
        if let Screen::Connecting { init_stage, .. } = &mut self.screen {
            *init_stage = stage
        }
    }

    fn tab(&mut self, backwards: bool) {
        if self.confirming_quit {
            self.quit_dialog.tab();
            return;
        }
        if let Screen::Login { screen, error } = &mut self.screen {
            if error.is_some() {
                return;
            }
            if self.show == Showing::Account {
                screen.account.tab();
            } else if self.show == Showing::Login {
                screen.banner.tab(backwards, self.server_field_locked);
            }
        }
    }

    fn request_quit(&mut self) {
        #[cfg(feature = "picasso-assets")]
        if let Some(job) = &self.asset_sync {
            // Cancel preparation, but never interrupt the atomic disk commit.
            job.cancel();
            return;
        }
        if !self.confirming_quit {
            self.quit_dialog.open();
            self.confirming_quit = true;
        }
    }
}

pub struct MainMenuUi {
    ui: Ui,
    // TODO: re add this
    // tip_no: u16,
    controls: Controls,
    bg_img_spec: &'static str,
}

impl MainMenuUi {
    pub fn new(global_state: &mut GlobalState) -> Self {
        #[cfg(target_os = "trueos")]
        {
            let size = global_state.window.window().surface_size();
            return Self::new_native(
                &global_state.settings,
                global_state.i18n,
                global_state.args.server.clone(),
                vek::Vec2::new(size.width, size.height),
                global_state.window.scale_factor(),
            );
        }
        #[cfg(not(target_os = "trueos"))]
        Self::new_gpu(global_state)
    }

    #[cfg(not(target_os = "trueos"))]
    fn new_gpu(global_state: &mut GlobalState) -> Self {
        // Load language
        let i18n = &global_state.i18n.read();
        // TODO: don't add default font twice
        let font = load_font(&i18n.fonts().get("cyri").unwrap().asset_key);

        let mut ui = Ui::new(
            &mut global_state.window,
            font,
            global_state.settings.interface.ui_scale,
        )
        .unwrap();

        let fonts = Fonts::load(i18n.fonts(), &mut ui).expect("Impossible to load fonts");

        let bg_img_spec = rand_bg_image_spec();

        let bg_img = Image::load_expect(bg_img_spec).read().to_image();
        let logo_glow = logo_glow::LogoGlow::new(&mut ui);
        let controls = Controls::new(
            fonts,
            Imgs::load(&mut ui).expect("Failed to load images"),
            ui.add_graphic(Graphic::Image(bg_img, None)),
            global_state.i18n,
            &global_state.settings,
            global_state.args.server.clone(),
            logo_glow,
        );

        Self {
            ui,
            controls,
            bg_img_spec,
        }
    }

    #[cfg(target_os = "trueos")]
    pub fn new_native(
        settings: &Settings,
        i18n: LocalizationHandle,
        server: Option<String>,
        resolution: vek::Vec2<u32>,
        scale_factor: f64,
    ) -> Self {
        let mut ui = Ui::new_native(resolution, scale_factor);
        let fonts = Fonts::load(i18n.read().fonts(), &mut ui).expect("Impossible to load fonts");
        let bg_img_spec = rand_bg_image_spec();
        let bg_img = Image::load_expect(bg_img_spec).read().to_image();
        let logo_glow = logo_glow::LogoGlow::new(&mut ui);
        let imgs = Imgs::load(&mut ui).expect("Failed to load images");
        let bg = ui.add_graphic(Graphic::Image(bg_img, None));
        ui.mark_scene_image(bg);
        ui.mark_scene_image(imgs.bg);
        let controls = Controls::new(fonts, imgs, bg, i18n, settings, server, logo_glow);
        Self {
            ui,
            controls,
            bg_img_spec,
        }
    }

    #[cfg(target_os = "trueos")]
    pub fn set_native_origin(&mut self, origin: vek::Vec2<f32>) {
        self.ui.set_native_origin(origin);
    }

    #[cfg(target_os = "trueos")]
    pub fn maintain_native(
        &mut self,
        settings: &Settings,
        runtime: &alloc::sync::Arc<tokio::runtime::Runtime>,
        clipboard: &mut ui::ice::Clipboard,
        resolution: vek::Vec2<u32>,
        dt: Duration,
    ) -> Result<(Vec<Event>, Option<ui::ice::renderer::bcs::FramePlan>), String> {
        #[cfg(feature = "picasso-assets")]
        self.poll_asset_sync();
        self.poll_account();
        let (messages, plan) = self.ui.maintain_native(
            self.controls.view(settings, dt.as_secs_f32()),
            resolution,
            clipboard,
        )?;
        let mut events = Vec::new();
        for message in messages {
            self.controls
                .update(message, &mut events, settings, &mut self.ui, runtime);
        }
        self.update_clipboard(clipboard);
        Ok((events, plan))
    }

    #[cfg(target_os = "trueos")]
    pub(crate) fn native_activity_screen(&self) -> &'static str {
        if self.controls.confirming_quit {
            return "quit";
        }
        match &self.controls.screen {
            Screen::Login { .. } => match self.controls.show {
                Showing::Login => "login",
                Showing::Languages => "languages",
                Showing::Account => "account",
            },
            Screen::Connecting { .. } => "connecting",
            #[cfg(feature = "picasso-assets")]
            Screen::AssetSync { .. } => "asset-sync",
            Screen::Servers { .. } => "servers",
            Screen::Credits { .. } => "credits",
        }
    }

    #[cfg(target_os = "trueos")]
    pub(crate) fn take_native_activity(
        &mut self,
    ) -> (
        ui::ice::renderer::activity::UiActivity,
        ui::ice::renderer::activity::PreparationActivity,
    ) {
        self.ui.take_native_activity()
    }

    /// Exercise the shipped loading screen without creating a client/world.
    #[cfg(target_os = "trueos")]
    pub fn show_loading_proof(&mut self) {
        self.controls.begin_connection(&mut self.ui);
    }

    pub fn bg_img_spec(&self) -> &'static str {
        self.bg_img_spec
    }

    pub fn update_language(&mut self, i18n: LocalizationHandle, settings: &Settings) {
        self.controls.i18n = i18n;
        let i18n = &i18n.read();
        let font = load_font(&i18n.fonts().get("cyri").unwrap().asset_key);
        self.ui.clear_fonts(font);
        self.controls.fonts =
            Fonts::load(i18n.fonts(), &mut self.ui).expect("Impossible to load fonts!");
        let language_metadatas = i18n::list_localizations();
        self.controls.selected_language_index = language_metadatas
            .iter()
            .position(|f| f.language_identifier == settings.language.selected_language);
    }

    pub fn auth_trust_prompt(&mut self, auth_server: String) {
        self.controls.auth_trust_prompt(auth_server);
    }

    pub fn show_info(&mut self, msg: String) {
        self.controls.connection_error(msg);
    }

    pub fn update_stage(&mut self, stage: DetailedInitializationStage) {
        tracing::trace!(?stage, "Updating stage");
        self.controls.update_init_stage(stage);
    }

    pub fn loading_minimum_elapsed(&self) -> bool {
        self.controls.loading_minimum_elapsed()
    }

    pub fn connected(&mut self) {
        self.controls.exit_connect_screen();
    }

    pub fn cancel_connection(&mut self) {
        self.controls.exit_connect_screen();
    }

    pub fn request_quit(&mut self) {
        self.controls.request_quit();
    }

    pub fn handle_event(&mut self, event: window::Event) -> bool {
        match event {
            // Pass events to ui.
            window::Event::IcedUi(event) => {
                self.handle_ui_event(event);
                true
            }
            window::Event::ScaleFactorChanged(s) => {
                self.ui.scale_factor_changed(s);
                false
            }
            _ => false,
        }
    }

    pub fn handle_ui_event(&mut self, event: ui::ice::Event) {
        use iced::keyboard;
        if self.controls.confirming_quit
            && matches!(
                &event,
                iced::Event::Keyboard(keyboard::Event::KeyPressed {
                    key_code: keyboard::KeyCode::Escape,
                    ..
                })
            )
        {
            self.controls.confirming_quit = false;
            return;
        }
        if let iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key_code: keyboard::KeyCode::Tab,
            modifiers,
        }) = &event
        {
            self.controls.tab(modifiers.shift);
            return;
        }
        if matches!(
            &event,
            iced::Event::Mouse(iced::mouse::Event::ButtonPressed(_))
                | iced::Event::Touch(iced::touch::Event::FingerPressed { .. })
        ) {
            if self.controls.confirming_quit {
                self.controls.quit_dialog.clear_focus();
            }
            if let Screen::Login { screen, .. } = &mut self.controls.screen {
                screen.banner.multiplayer_focus.focus(false);
            }
        }

        self.ui.handle_event(event);
    }

    pub fn set_scale_mode(&mut self, scale_mode: ui::ScaleMode) {
        self.ui.set_scaling_mode(scale_mode);
    }

    #[cfg(target_os = "trueos")]
    pub(crate) fn invalidate_native(&mut self) {
        self.ui.invalidate_native();
    }

    /// Use the normal Multiplayer path once per process, including its loading
    /// minimum and cancellation behavior. Returning to this menu never retries.
    #[cfg(target_os = "trueos")]
    pub(crate) fn startup_login(&mut self) -> Option<Event> {
        use core::sync::atomic::{AtomicBool, Ordering};
        static ATTEMPTED: AtomicBool = AtomicBool::new(false);
        if ATTEMPTED.swap(true, Ordering::Relaxed) {
            return None;
        }
        let mut password = match trueos::async_fs::block_on(
            trueos::async_fs::read_file_utf8(b"/apps/voxy/voxy.pw"),
        ) {
            Ok(password) => password,
            Err(code) => {
                tracing::info!(code, "Startup login skipped: password file unavailable");
                return None;
            }
        };
        // Preserve password spaces; strip only the file's trailing line endings.
        password.truncate(password.trim_end_matches(['\r', '\n']).len());
        if password.is_empty() || password.len() > 4096 || password.chars().any(char::is_control) {
            tracing::warn!("Startup login skipped: invalid password file");
            return None;
        }
        if self.controls.login_info.username.trim().is_empty()
            || self.controls.login_info.server.trim().is_empty()
        {
            tracing::warn!("Startup login skipped: username or server missing");
            return None;
        }
        self.controls.login_info.password = password;
        self.controls.begin_connection(&mut self.ui);
        tracing::info!("Automatic Multiplayer login from /apps/voxy/voxy.pw");
        Some(self.controls.login_attempt())
    }

    pub fn maintain(&mut self, global_state: &mut GlobalState, dt: Duration) -> Vec<Event> {
        #[cfg(target_os = "trueos")]
        {
            let size = global_state.window.window().surface_size();
            let size = vek::Vec2::new(size.width, size.height);
            if size.x == 0 || size.y == 0 {
                return Vec::new();
            }
            use winit::platform::trueos::WindowExtTrueOS;
            let viewport = global_state.window.window().trueos_content_viewport();
            self.set_native_origin(vek::Vec2::new(
                viewport.position.x as f32,
                viewport.position.y as f32,
            ));
            return match self.maintain_native(
                &global_state.settings,
                &global_state.tokio_runtime,
                &mut global_state.clipboard,
                vek::Vec2::new(viewport.size.width, viewport.size.height),
                dt,
            ) {
                Ok((events, plan)) => {
                    if let Some(plan) = plan {
                        if let Err(error) = global_state.window.present_menu(size, plan) {
                            global_state.info_message = Some(error);
                        }
                    }
                    events
                }
                Err(error) => {
                    global_state.info_message = Some(error);
                    Vec::new()
                }
            };
        }
        #[cfg(not(target_os = "trueos"))]
        self.maintain_gpu(global_state, dt)
    }

    #[cfg(not(target_os = "trueos"))]
    fn maintain_gpu(&mut self, global_state: &mut GlobalState, dt: Duration) -> Vec<Event> {
        let mut events = Vec::new();

        #[cfg(feature = "picasso-assets")]
        self.poll_asset_sync();
        self.poll_account();

        let (messages, _) = self.ui.maintain(
            self.controls.view(&global_state.settings, dt.as_secs_f32()),
            global_state.window.renderer_mut(),
            None,
            &mut global_state.clipboard,
        );

        messages.into_iter().for_each(|message| {
            self.controls.update(
                message,
                &mut events,
                &global_state.settings,
                &mut self.ui,
                &global_state.tokio_runtime,
            )
        });

        self.update_clipboard(&mut global_state.clipboard);
        events
    }

    #[cfg(feature = "picasso-assets")]
    fn poll_asset_sync(&mut self) {
        let Some(result) = self
            .controls
            .asset_sync
            .as_ref()
            .and_then(|job| job.status().result)
        else {
            return;
        };
        self.controls.asset_sync = None;
        if let Some(previous) = self.controls.asset_sync_previous.take() {
            self.controls.screen = *previous;
        }
        if matches!(&result, Err(message) if message == "Asset sync canceled") {
            return;
        }
        let message = result.unwrap_or_else(|error| format!("Asset Sync failed: {error}"));
        self.controls.show = Showing::Login;
        if let Screen::Login { error, .. } = &mut self.controls.screen {
            *error = Some(message);
        } else {
            self.controls.screen = Screen::Login {
                screen: Box::default(),
                error: Some(message),
            };
        }
    }

    fn poll_account(&mut self) {
        if let Screen::Login { screen, error } = &mut self.controls.screen {
            if let Some(result) = screen.account.poll() {
                let i18n = self.controls.i18n.read();
                screen.account_created = result.is_ok();
                *error = Some(match result {
                    Ok(username) => {
                        self.controls.login_info.username = username;
                        self.controls.login_info.password.clear();
                        i18n.get_msg("main-account-created").into_owned()
                    }
                    Err(response) => {
                        format!("{}\n{}", i18n.get_msg("main-account-failed"), response)
                    }
                });
                self.controls.show = Showing::Account;
            }
        }
    }

    fn update_clipboard(&mut self, clipboard: &mut ui::ice::Clipboard) {
        if let Screen::Login { screen, .. } = &self.controls.screen {
            if self.controls.show == Showing::Login {
                if screen.banner.password.is_focused() {
                    clipboard.focus(crate::clipboard::Kind::Password);
                } else if screen.banner.username.is_focused() || screen.banner.server.is_focused() {
                    clipboard.focus(crate::clipboard::Kind::Text);
                } else {
                    clipboard.blur();
                }
            }
        } else {
            clipboard.blur();
        }
        if let Some(message) = clipboard.take_message() {
            if let Screen::Login { error, .. } = &mut self.controls.screen {
                *error = Some(message);
            }
        }
    }

    pub fn render<'a>(&'a self, drawer: &mut UiDrawer<'_, 'a>) {
        self.ui.render(drawer);
    }
}

pub fn rand_bg_image_spec() -> &'static str {
    BG_IMGS.choose(&mut rng()).unwrap()
}
