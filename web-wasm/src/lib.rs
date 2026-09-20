use mmorpg_core::{PlayerId, TICK_HZ, ZoneCommand, ZoneId, ZoneSimulation};
use wasm_bindgen::prelude::*;

const DEMO_ZONE_ID: u32 = 1;
const DEMO_PLAYER_ID: PlayerId = 1;
const MAX_ADVANCE_TICKS: u32 = 8;

#[wasm_bindgen]
pub fn tick_hz() -> u16 {
    TICK_HZ
}

#[wasm_bindgen]
pub struct DemoSimulation {
    zone: ZoneSimulation,
    sequence: u32,
    movement_x: i8,
    movement_z: i8,
}

#[wasm_bindgen]
impl DemoSimulation {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Result<DemoSimulation, JsValue> {
        Self::create()
    }

    pub fn reset(&mut self) -> Result<(), JsValue> {
        *self = Self::create()?;
        Ok(())
    }

    pub fn set_movement(&mut self, x: i8, z: i8) -> Result<(), JsValue> {
        if self.movement_x == x && self.movement_z == z {
            return Ok(());
        }

        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or_else(|| js_error("demo input sequence overflow"))?;
        self.zone
            .apply_command(DEMO_PLAYER_ID, self.sequence, ZoneCommand::SetMovement { x, z })
            .map_err(js_error)?;
        self.movement_x = x;
        self.movement_z = z;
        Ok(())
    }

    pub fn advance_ticks(&mut self, ticks: u32) -> Result<(), JsValue> {
        if ticks > MAX_ADVANCE_TICKS {
            return Err(js_error("browser frame requested too many simulation ticks"));
        }
        for _ in 0..ticks {
            self.zone.advance_tick().map_err(js_error)?;
        }
        Ok(())
    }

    pub fn snapshot_json(&self) -> Result<String, JsValue> {
        let snapshot = self.zone.snapshot().map_err(js_error)?;
        let player = snapshot
            .players
            .iter()
            .find(|player| player.player_id == DEMO_PLAYER_ID)
            .ok_or_else(|| js_error("demo player is missing from the authoritative zone"))?;

        Ok(format!(
            "{{\"tick\":{},\"position\":[{},{},{}]}}",
            snapshot.tick, player.position[0], player.position[1], player.position[2]
        ))
    }
}

impl DemoSimulation {
    fn create() -> Result<Self, JsValue> {
        let mut zone = ZoneSimulation::new(ZoneId::new(DEMO_ZONE_ID));
        zone.add_player(DEMO_PLAYER_ID).map_err(js_error)?;
        Ok(Self {
            zone,
            sequence: 0,
            movement_x: 0,
            movement_z: 0,
        })
    }
}

fn js_error(error: impl ToString) -> JsValue {
    JsValue::from_str(&error.to_string())
}
