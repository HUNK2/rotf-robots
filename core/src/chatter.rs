//! Bumblebee's battle chatter table and the original single-speaker selection rules.
//! [game] 007cc400/007cc720: match action/category/either subcategory, choose uniformly,
//! then test the chosen line's frequency. 007cd880 history compares the three hashes,
//! not the waveform, and 007cd430 applies a randomized speak delay.
use std::{collections::HashMap, path::Path};
use crate::formats::{hash::crc32, lxb::{DataFile, Node}, pack::{self, Pack}};

pub const ATTACK: u32 = crc32(b"CHATTER_ACTION_ATTACK");
pub const DIED: u32 = crc32(b"CHATTER_ACTION_DIED");
pub const INJURED: u32 = crc32(b"CHATTER_ACTION_INJURED");
pub const CHARACTER: u32 = crc32(b"CHATTER_CATEGORY_CHARACTER");
pub const GENERIC: u32 = crc32(b"CHATTER_CATEGORY_GENERIC");
pub const SELF: u32 = crc32(b"CHATTER_SUBCATEGORY_SELF");
pub const GENERIC_SUB: u32 = crc32(b"CHATTER_SUBCATEGORY_GENERIC");
pub const ENEMY: u32 = crc32(b"CHATTER_SUBCATEGORY_ENEMY");

#[derive(Clone, Copy, Debug)]
pub struct Line { pub action: u32, pub category: u32, pub subcategory: u32, pub event: u32, pub frequency: f32 }
pub fn read(node: Node) -> Vec<Line> {
    node.fields().into_iter().filter_map(|(_, n)| {
        let hash = |field| n.get(field).and_then(Node::int).map(|v| v as u32);
        Some(Line { action: hash("action")?, category: hash("category")?, subcategory: hash("subCategory")?,
            event: hash("eventID")?, frequency: n.get("frequency").and_then(Node::float)? })
    }).collect()
}

pub struct Tuning {
    pub enabled: bool,
    pub low_health: f32,
    pub delay: [f32; 2],
    history: HashMap<u32, f32>,
}
impl Tuning {
    pub fn load(game: &Path) -> Result<Self, String> {
        let pack = Pack::open(&game.join("bnxglobal.str"))?;
        for chunk in pack.of_type(pack::DATA) {
            let Ok(file) = DataFile::parse(pack.data(chunk).to_vec()) else { continue };
            let Some(node) = file.root().get("BattleChatter") else { continue };
            let value = |field| node.get(field).and_then(Node::float).ok_or_else(|| format!("BattleChatter.{field} missing"));
            let mut history = HashMap::new();
            for (action, field) in [
                (ATTACK, "attackHistoryTime"), (DIED, "diedHistoryTime"), (INJURED, "injuredHistoryTime"),
                (crc32(b"CHATTER_ACTION_MOVETO"), "moveToHistoryTime"), (crc32(b"CHATTER_ACTION_SPOT"), "spotHistoryTime"),
                (crc32(b"CHATTER_ACTION_TAUNT"), "tauntHistoryTime"), (crc32(b"CHATTER_ACTION_TACTICS"), "tacticsHistoryTime"),
            ] { history.insert(action, value(field)?); }
            return Ok(Self { enabled: node.get("startEnabled").and_then(Node::int) == Some(1),
                low_health: value("charLowHealthPct")?, delay: [value("minSpeakDelay")?, value("maxSpeakDelay")?], history });
        }
        Err("bnxglobal.str has no BattleChatter tuning".into())
    }
}

pub struct Speaker {
    pub lines: Vec<Line>,
    pub tuning: Tuning,
    history: HashMap<(u32, u32, u32), f32>,
}
impl Speaker {
    pub fn new(lines: Vec<Line>, tuning: Tuning) -> Self { Self { lines, tuning, history: HashMap::new() } }
    pub fn step(&mut self, dt: f32) { self.history.retain(|_, left| { *left -= dt; *left >= 0.0 }); }
    pub fn clear(&mut self) { self.history.clear(); }
    pub fn choose(&mut self, action: u32, category: u32, subcategories: [u32; 2], random: &mut impl FnMut() -> f32) -> Option<Line> {
        // One stream for both draws: the line first, then the frequency share.
        let random = std::cell::RefCell::new(random);
        self.choose_with(action, category, subcategories,
            &mut |n| ((random.borrow_mut())().clamp(0.0, 0.999999) * n as f32) as usize,
            &mut || (random.borrow_mut())())
    }

    /// The game's roll (007cc720, 007cd430): the line by `random_range` over [0, n) (011904f0), then the
    /// frequency test `random_range_with_clock(0, 1) <= frequency` (00551d60). [game]
    pub fn choose_with(&mut self, action: u32, category: u32, subcategories: [u32; 2],
        pick: &mut impl FnMut(usize) -> usize, share: &mut impl FnMut() -> f32) -> Option<Line> {
        if !self.tuning.enabled { return None; }
        let candidates: Vec<_> = self.lines.iter().filter(|l| l.action == action && l.category == category
            && subcategories.contains(&l.subcategory)).collect();
        if candidates.is_empty() { return None; }
        let chosen = *candidates[pick(candidates.len()).min(candidates.len() - 1)];
        if share() > chosen.frequency { return None; }
        let key = (chosen.action, chosen.category, chosen.subcategory);
        if self.history.contains_key(&key) { return None; }
        self.history.insert(key, self.tuning.history.get(&action).copied().unwrap_or(0.0));
        Some(chosen)
    }
}

/// The game's random numbers for the chatter: one state, the C library's LCG (x 0x343fd + 0x269ec3),
/// with the clock's seconds (`_time64`) mixed into every draw. `range` is `random_range` 011904f0
/// (`((s >> 16) ^ t) & 0x7fff` modulo n), `between` is `random_range_with_clock` 00551d60 (fifteen bits of
/// `(s >> 8) ^ (t << 8)` as a share of [a, b), the state stepped first). [game] The game's state is shared
/// with every other caller of the two, so its sequence cannot be reproduced; the generator and its ranges
/// are the game's.
#[derive(Clone, Copy, Debug)]
pub struct GameRandom(pub u32);

impl GameRandom {
    fn step(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(0x343fd).wrapping_add(0x26_9ec3);
        self.0
    }
    pub fn range(&mut self, n: usize, seconds: u32) -> usize {
        if n == 0 { return 0; }
        ((((self.step() >> 16) ^ seconds) & 0x7fff) as usize) % n
    }
    pub fn between(&mut self, low: f32, high: f32, seconds: u32) -> f32 {
        let share = f32::from_bits(((self.step() >> 8) ^ seconds.wrapping_shl(8)) & 0x7f_ff00 | 0x3f80_0000) - 1.0;
        low + (high - low) * share
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn game_random_draws_stay_in_their_ranges() {
        let mut r = GameRandom(1);
        for t in 0..2000u32 {
            assert!(r.range(7, t) < 7);
            let x = r.between(2.0, 5.0, t);
            assert!((2.0..5.0).contains(&x), "{x}");
        }
        // The first draw from state 1 with the clock at 0: s = 0x343fd + 0x269ec3 = 0x29e2c0.
        let mut r = GameRandom(1);
        assert_eq!(r.range(1 << 15, 0), (0x29e2c0 >> 16) & 0x7fff);
    }
    #[test]
    fn frequency_and_group_history_apply_to_the_selected_line() {
        let mut speaker = Speaker::new(vec![
            Line { action: ATTACK, category: GENERIC, subcategory: GENERIC_SUB, event: 1, frequency: 0.333 },
            Line { action: ATTACK, category: GENERIC, subcategory: GENERIC_SUB, event: 2, frequency: 0.333 },
            Line { action: INJURED, category: CHARACTER, subcategory: SELF, event: 3, frequency: 1.0 },
        ], Tuning { enabled: true, low_health: 0.333, delay: [0.3, 1.5], history: HashMap::from([(ATTACK, 25.0), (INJURED, 25.0)]) });
        assert!(speaker.choose(ATTACK, GENERIC, [GENERIC_SUB, 0], &mut || 0.5).is_none());
        assert_eq!(speaker.choose(ATTACK, GENERIC, [GENERIC_SUB, 0], &mut || 0.1).unwrap().event, 1);
        assert!(speaker.choose(ATTACK, GENERIC, [GENERIC_SUB, 0], &mut || 0.2).is_none(), "history blocks the whole group");
        assert!(speaker.choose(INJURED, CHARACTER, [SELF, 0], &mut || 0.1).is_some());
        speaker.step(25.01);
        assert!(speaker.choose(ATTACK, GENERIC, [GENERIC_SUB, 0], &mut || 0.1).is_some());
    }
    #[test]
    fn installed_chatter_tuning_is_read_without_fitting() {
        if crate::install::missing() { return; }
        let t = Tuning::load(crate::install::dir()).unwrap();
        assert!(t.enabled);
        assert_eq!((t.low_health, t.delay, t.history[&ATTACK]), (0.333, [0.3, 1.5], 25.0));
    }
}
