use crate::core::ClientEntities;
use crate::events::{BrokerCommand, BrokerEvent};
use crate::player::{Ghost, Player, PlayerBundle};
use crate::states::ServerState;
use bevy::prelude::*;
use bevy::app::AppExit;
use rand::Rng;
use shared::custom_id::CustomId;
use shared::game_protocol::{PlayerData, WorldSyncPayload};
use shared::models::Vec2 as MathVec2;
pub struct GamePlugin;

impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PendingShutdown>()
            .add_systems(
                Update,
                (
                    handle_broker_events,
                    broadcast_sync_to_clients,
                    send_spatial_updates,
                    check_graceful_shutdown,
                )
                    .chain()
                    .run_if(in_state(ServerState::Active)),
            );
    }
}

#[derive(Component)]
pub struct NetworkClient {
    pub id: CustomId,
}

#[derive(Component)]
pub struct SpatialSync {
    pub timer: Timer,
}

#[derive(Resource, Default)]
pub struct PendingShutdown(pub bool);

impl Default for SpatialSync {
    fn default() -> Self {
        let mut timer = Timer::from_seconds(1.0, TimerMode::Repeating);
        let random_offset = rand::rng().random_range(0.0..1.0);
        timer.tick(std::time::Duration::from_secs_f32(random_offset));
        Self { timer }
    }
}

// ✨ Le composant public qui garde en mémoire ce que veut faire le joueur
#[derive(Component, Default)]
pub struct PlayerInputState {
    pub latest_input: u8,
}

fn handle_broker_events(
    mut commands: Commands,
    mut ev_broker: MessageReader<BrokerEvent>,
    mut ev_commands: MessageWriter<BrokerCommand>,
    mut client_entities: ResMut<ClientEntities>,
    mut q_inputs: Query<&mut PlayerInputState, Without<Ghost>>,
) {
    for ev in ev_broker.read() {
        match ev {
            BrokerEvent::SpawnPlayer { client_id, pos } => {
                let id_u32: u32 = (*client_id).into();
                let entity = commands
                    .spawn((
                        PlayerBundle::new(*pos, 100, 300.0, 15.0),
                        NetworkClient { id: *client_id },
                        SpatialSync::default(),
                        PlayerInputState::default(), // On attache l'état vierge
                    ))
                    .id();

                client_entities.0.insert(id_u32, entity);
                info!("👤 Autorité prise sur le Joueur {} en {:?} !", id_u32, pos);
            }

            BrokerEvent::PlayerLeft { client_id } => {
                let id_u32: u32 = (*client_id).into();
                if let Some(entity) = client_entities.0.remove(&id_u32) {
                    commands.entity(entity).despawn();
                    info!("👋 Joueur {} a quitté la partie !", id_u32);
                }
            }

            BrokerEvent::PlayerInput { client_id, payload } => {
                let id_u32: u32 = (*client_id).into();
                if let Some(&entity) = client_entities.0.get(&id_u32) {
                    if let Ok(mut input_state) = q_inputs.get_mut(entity) {
                        input_state.latest_input = payload.inputs[0].input;
                    }
                }
            }

            BrokerEvent::SpawnGhostPlayer { shard_id, client_id } => {
                let id_u32: u32 = (*client_id).into();
                let entity = commands
                    .spawn((
                        PlayerBundle::new(Vec2::ZERO, 100, 300.0, 15.0),
                        Ghost,
                        NetworkClient { id: *client_id },
                        SpatialSync::default(),
                        PlayerInputState::default(),
                    ))
                    .id();

                client_entities.0.insert(id_u32, entity);

                info!("👻 Joueur {} est passé en ghost (entité {:?}) !", id_u32, entity);
                ev_commands.write(BrokerCommand::SendHandoffAccept {
                    shard_id: *shard_id,
                    entity_id: *client_id,
                });
            }

            BrokerEvent::TakeAuthority { client_id, new_pos } => {
                let id_u32: u32 = (*client_id).into();

                if let Some(&entity) = client_entities.0.get(&id_u32) {
                    commands.entity(entity)
                        .remove::<Ghost>()
                        .insert(Transform::from_translation(Vec3::new(new_pos.x, new_pos.y, 0.0)));

                    info!("🎮 Autorité reprise sur le joueur {} (ghost retiré, position mise à jour)", id_u32);
                }
            }

            BrokerEvent::DropAuthority { client_id } => {
                let id_u32: u32 = (*client_id).into();
                if let Some(&entity) = client_entities.0.get(&id_u32) {
                    commands.entity(entity).insert(Ghost);
                    info!("👻 Autorité lâchée sur le joueur {} (ghost ajouté)", id_u32);
                }
            }

            BrokerEvent::ShutdownRequested => {
                commands.insert_resource(PendingShutdown(true));
            }

            BrokerEvent::HandoffDrop{ client_id } => {
                let id_u32: u32 = (*client_id).into();
                if let Some(&entity) = client_entities.0.get(&id_u32) {
                    commands.entity(entity).despawn();
                    info!("🪓 Entité {} tuée", id_u32);
                }
            }
        }
    }
}

fn broadcast_sync_to_clients(
    query: Query<(&Transform, &NetworkClient), (With<Player>, Without<Ghost>)>,
    mut ev_commands: MessageWriter<BrokerCommand>,
) {
    if query.is_empty() { return; }

    let entities_data: Vec<PlayerData> = query
        .iter()
        .map(|(transform, client)| {
            let pos = transform.translation.truncate();
            PlayerData {
                client_id: client.id,
                pos: (pos.x, pos.y),
            }
        })
        .collect();

    let sync_payload = WorldSyncPayload { entities: entities_data };
    ev_commands.write(BrokerCommand::SendWorldSync(sync_payload));
}

fn send_spatial_updates(
    time: Res<Time>,
    mut query: Query<(&mut SpatialSync, &NetworkClient, &Transform), (With<Player>, Without<Ghost>)>,
    mut ev_commands: MessageWriter<BrokerCommand>,
) {
    for (mut sync, client, transform) in query.iter_mut() {
        if sync.timer.tick(time.delta()).just_finished() {
            let pos = transform.translation.truncate();
            ev_commands.write(BrokerCommand::SendPositionUpdate {
                client_id: client.id,
                pos: MathVec2::new(pos.x, pos.y),
            });
        }
    }
}

fn check_graceful_shutdown(
    pending: Res<PendingShutdown>,
    query: Query<(), (With<Player>, Without<Ghost>)>,
    mut exit: MessageWriter<AppExit>,
) {
    if pending.0 && query.is_empty() {
        info!("🏁 Serveur vide et en attente de fermeture. Extinction...");
        exit.write(AppExit::Success);
    }
}