//! Prepare an ordinary authenticated client while the source world remains alive.
use crate::{
    GlobalState,
    client::{Client, Event, addr::ConnectionArgs},
    menu::main::client_init::{ClientInit, Msg},
    portal_timeline::{CameraStage, Cinematic},
};
use common::{comp, event::UpdateCharacterMetadata};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use vek::Vec3;
use zeroize::Zeroizing;

pub const OFFICIAL_SERVER: &str = "server.veloren.net";
pub const OFFICIAL_AUTH_SERVER: &str = "https://auth.veloren.net";

/// Deliberately has no Debug or Serialize implementation.
pub struct PortalCredentials {
    username: String,
    password: Zeroizing<String>,
}
impl PortalCredentials {
    pub fn new(username: String, password: String) -> Self {
        Self {
            username,
            password: Zeroizing::new(password),
        }
    }
}
#[derive(Clone, Copy)]
pub struct PortalCamera {
    pub orientation: Vec3<f32>,
    pub distance: f32,
    pub first_person: bool,
}

pub struct PortalTransfer {
    pub id: u64,
    pub hover_position: Vec3<f32>,
    init: Option<ClientInit>,
    destination: Option<Client>,
    runtime: Arc<tokio::runtime::Runtime>,
    started: Instant,
    requested_character: bool,
    metadata: Option<UpdateCharacterMetadata>,
    pub cinematic: Option<Cinematic>,
    facing: f32,
    pub saved_camera: PortalCamera,
    pub snapshot: Option<crossbeam_channel::Receiver<Result<image::RgbImage, String>>>,
}
impl PortalTransfer {
    pub fn begin(
        id: u64,
        destination: &str,
        hover_position: Vec3<f32>,
        global: &GlobalState,
        saved_camera: PortalCamera,
        facing: f32,
    ) -> Result<Self, String> {
        // A portal cannot redirect retained credentials to an arbitrary auth provider.
        // More destinations can be added later through explicit account/trust selection.
        if destination != OFFICIAL_SERVER {
            return Err("This portal destination is not configured for this account.".into());
        }
        let credentials = global
            .portal_credentials
            .as_ref()
            .filter(|c| !c.username.is_empty() && !c.password.is_empty())
            .ok_or("Log in with your Veloren account password before using the town portal.")?;
        let init = ClientInit::new_portal(
            ConnectionArgs::Srv {
                hostname: destination.into(),
                prefer_ipv6: false,
                validate_tls: true,
                use_quic: global.settings.networking.use_quic,
            },
            credentials.username.clone(),
            credentials.password.to_string(),
            Arc::clone(&global.tokio_runtime),
            global
                .settings
                .language
                .send_to_server
                .then(|| global.settings.language.selected_language.clone()),
            &global.config_dir,
            common_net::msg::ClientType::Game,
        );
        Ok(Self {
            id,
            hover_position,
            init: Some(init),
            destination: None,
            runtime: Arc::clone(&global.tokio_runtime),
            started: Instant::now(),
            requested_character: false,
            metadata: None,
            cinematic: None,
            facing,
            saved_camera,
            snapshot: None,
        })
    }
    /// True only after character admission, ECS sync and the surrounding terrain arrive.
    pub fn poll(&mut self, global: &GlobalState, dt: Duration) -> Result<bool, String> {
        if self.started.elapsed() > Duration::from_secs(45) && self.cinematic.is_none() {
            return Err(
                "The destination did not become ready. You are still in your home world.".into(),
            );
        }
        if let Some(init) = &self.init {
            if let Some(message) = init.poll() {
                match message {
                    Msg::IsAuthTrusted(provider) => {
                        init.auth_trust(provider.clone(), provider == OFFICIAL_AUTH_SERVER)
                    }
                    Msg::Done(Err(_)) => {
                        return Err(
                            "Could not authenticate or connect to the destination server.".into(),
                        );
                    }
                    Msg::Done(Ok(mut client)) => {
                        if client.server_info().auth_provider.as_deref()
                            != Some(OFFICIAL_AUTH_SERVER)
                        {
                            return Err(
                                "The destination did not use official Veloren authentication."
                                    .into(),
                            );
                        }
                        if client.are_plugins_missing() {
                            return Err("The destination requires unavailable plugins.".into());
                        }
                        crate::ecs::init(client.state_mut().ecs_mut());
                        client.load_character_list();
                        self.destination = Some(client);
                        self.init = None;
                    }
                }
            }
        }
        if let Some(client) = &mut self.destination {
            let events = client.tick(comp::ControllerInputs::default(), dt).map_err(
                |_| "The destination connection was lost. You remain in your home world.",
            )?;
            if client.is_dead()
                || comp::is_downed(client.current().as_ref(), client.current().as_ref())
            {
                return Err("Your destination character is no longer ready to enter.".into());
            }
            for event in events {
                match event {
                    Event::CharacterJoined(metadata) => self.metadata = Some(metadata),
                    Event::CharacterError(_) | Event::Disconnect => {
                        return Err("The destination could not admit your character.".into());
                    }
                    _ => {}
                }
            }
            if !self.requested_character && !client.character_list().loading {
                let preferred = global.profile.get_selected_character(OFFICIAL_SERVER);
                let ids = client
                    .character_list()
                    .characters
                    .iter()
                    .filter_map(|c| c.character.id)
                    .collect::<Vec<_>>();
                let id = preferred
                    .filter(|id| ids.contains(id))
                    .or_else(|| ids.first().copied())
                    .ok_or(
                        "Create a character on the destination server before using this portal.",
                    )?;
                let graphics = &global.settings.graphics;
                client.request_character(
                    id,
                    common::ViewDistances {
                        terrain: graphics.terrain_view_distance,
                        entity: graphics.entity_view_distance,
                    },
                );
                self.requested_character = true;
            }
            let ready = self.metadata.is_some()
                && client.current::<comp::Body>().is_some()
                && client.current::<comp::CharacterState>().is_some()
                && client.position().is_some_and(|pos| {
                    let p = pos.map(|v| v.floor() as i32);
                    [-8, 0, 8].into_iter().all(|x| {
                        [-8, 0, 8].into_iter().all(|y| {
                            client
                                .state()
                                .terrain()
                                .get_key_arc_real(common::terrain::TerrainGrid::chunk_key(
                                    p + Vec3::new(x, y, 0),
                                ))
                                .is_some()
                        })
                    })
                });
            client.cleanup();
            if ready && self.cinematic.is_none() {
                self.cinematic = Some(Cinematic::default());
            }
        }
        if let Some(cinematic) = &mut self.cinematic {
            cinematic.advance(dt.as_secs_f32());
        }
        Ok(self
            .cinematic
            .as_ref()
            .is_some_and(Cinematic::ready_to_swap))
    }
    pub fn facing_direction(&self) -> Vec3<f32> {
        Vec3::new(self.facing.sin(), self.facing.cos(), 0.0)
    }

    pub fn camera(&self) -> Option<PortalCamera> {
        let c = self.cinematic.as_ref()?;
        let p = Cinematic::ramp(c.progress());
        use CameraStage::*;
        Some(match c.stage {
            FadeToBlack => self.saved_camera,
            FaceBlackHold | FaceReveal => PortalCamera {
                orientation: Vec3::new(self.facing + std::f32::consts::PI, 0.0, 0.0),
                distance: 7.0,
                first_person: false,
            },
            FaceApproach => PortalCamera {
                orientation: Vec3::new(self.facing + std::f32::consts::PI, 0.0, 0.0),
                distance: 7.0 - 5.0 * p,
                first_person: false,
            },
            PerspectiveBlackHold => PortalCamera {
                orientation: Vec3::new(self.facing, 0.0, 0.0),
                distance: 0.0,
                first_person: true,
            },
            LookUp => PortalCamera {
                orientation: Vec3::new(self.facing, -std::f32::consts::FRAC_PI_2 * 0.95 * p, 0.0),
                distance: 0.0,
                first_person: true,
            },
            WhiteHold => PortalCamera {
                orientation: Vec3::new(self.facing, -std::f32::consts::FRAC_PI_2 * 0.95, 0.0),
                distance: 0.0,
                first_person: true,
            },
            ArrivalReveal | Complete => return None,
        })
    }
    pub fn take_destination(&mut self) -> (Client, UpdateCharacterMetadata) {
        (
            self.destination.take().expect("admitted destination"),
            self.metadata.take().expect("character admission"),
        )
    }
}
impl Drop for PortalTransfer {
    fn drop(&mut self) {
        if let Some(client) = self.destination.take() {
            self.runtime.spawn_blocking(move || drop(client));
        }
    }
}

/// Restores hardware gamma even if a session exits during a cinematic.
pub struct DisplayFade {
    #[cfg(target_os = "trueos")]
    window_id: u32,
}
impl DisplayFade {
    pub fn new(window: &crate::window::Window) -> Self {
        #[cfg(target_os = "trueos")]
        {
            use winit::platform::trueos::WindowExtTrueOS;
            Self {
                window_id: window.window().trueos_window_id(),
            }
        }
        #[cfg(not(target_os = "trueos"))]
        {
            let _ = window;
            Self {}
        }
    }
    pub fn set(&self, amount: f32) -> Result<(), String> {
        #[cfg(target_os = "trueos")]
        {
            trueos::ui4_scene::display_fade(self.window_id, amount)
                .map_err(|e| format!("Display fade unavailable: {e:?}"))
        }
        #[cfg(not(target_os = "trueos"))]
        {
            let _ = amount;
            Ok(())
        }
    }
}
impl Drop for DisplayFade {
    fn drop(&mut self) {
        let _ = self.set(0.0);
    }
}
