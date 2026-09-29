//! 封面的群系：建实例时挑一个，记下来。
//!
//! 封面由界面画（`fern-kit/src/ui/biome.ts`），群系原本也由它按种子哈希取模
//! 决定。8 套色板，6 个实例里至少两个撞色的概率约 92%——而封面是实例的脸，
//! 两个实例长一样就失去了它唯一的用处。
//!
//! 所以群系在建实例的那一刻挑定：挑现有实例用得最少的那一个，并列时照旧用哈
//! 希的结果，于是第一个实例和以前长得一模一样。挑定之后写进 `instance.json`，
//! 以后删掉别的实例也不会让它变脸。没记群系的老实例照旧按哈希算，外观不变。
//!
//! 群系表和哈希是 `biome.ts` 的复刻，两边必须一致；测试会读那个文件核对。

use super::{CoverSeed, InstanceProfile};

/// 与 `biome.ts` 里 `BIOMES` 的键同序——哈希取模的结果依赖这个顺序。
pub(crate) const BIOMES: [&str; 8] = [
    "ocean", "jungle", "nether", "end", "badland", "snowy", "swamp", "cherry",
];

/// FNV-1a，按 UTF-16 码元走，和 `biome.ts` 的 `hash32` 逐位相同
/// （那边用的是 `charCodeAt`）。
fn hash32(text: &str) -> u32 {
    text.encode_utf16().fold(2_166_136_261u32, |hash, unit| {
        (hash ^ u32::from(unit)).wrapping_mul(16_777_619)
    })
}

/// 没记群系时，界面会按种子算出的那一个。
fn implied(identity: &str) -> usize {
    // 空种子在界面那边按 'x' 算。
    let seed = if identity.is_empty() { "x" } else { identity };
    hash32(seed) as usize % BIOMES.len()
}

/// 这张封面实际是哪个群系。
fn biome_of(cover: &CoverSeed) -> usize {
    cover
        .biome
        .as_deref()
        .and_then(|name| BIOMES.iter().position(|key| *key == name))
        .unwrap_or_else(|| implied(&cover.identity))
}

/// 给一个新实例挑群系：现有实例里用得最少的那个。
///
/// 从这个实例自己的哈希结果开始往后找，所以并列时就是它本来会得到的那一个。
pub(crate) fn pick(existing: &[InstanceProfile], identity: &str) -> String {
    let mut used = [0usize; BIOMES.len()];
    for profile in existing {
        used[biome_of(&profile.cover)] += 1;
    }
    let fewest = used.iter().copied().min().unwrap_or(0);
    let start = implied(identity);
    let chosen = (0..BIOMES.len())
        .map(|offset| (start + offset) % BIOMES.len())
        .find(|&index| used[index] == fewest)
        .unwrap_or(start);
    BIOMES[chosen].to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::InstanceId;

    fn profile(id: &str, biome: Option<&str>) -> InstanceProfile {
        let mut profile =
            InstanceProfile::vanilla(InstanceId::parse(id).expect("id"), id, "1.21.1");
        profile.cover.biome = biome.map(str::to_owned);
        profile
    }

    /// 期望值是用 `biome.ts` 的 `resolve({ name }).bk` 当场算出来的。
    #[test]
    fn the_hash_agrees_with_the_interface() {
        for (seed, biome) in [
            ("", "cherry"),
            ("x", "cherry"),
            ("create", "snowy"),
            ("k3v9q2mz", "nether"),
            ("1.12.2 怀旧", "nether"),
            ("空岛", "nether"),
            ("Fern", "badland"),
            ("😀ab", "end"),
        ] {
            assert_eq!(BIOMES[implied(seed)], biome, "{seed:?}");
        }
    }

    #[test]
    fn the_biome_table_matches_the_interface() {
        let source = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../fern-kit/src/ui/biome.ts"),
        )
        .expect("read biome.ts");
        let table = &source[source.find("export const BIOMES = {").expect("BIOMES")..];
        let table = &table[..table.find("} as const").expect("end of BIOMES")];
        let keys: Vec<&str> = table
            .lines()
            .filter_map(|line| line.trim().split_once(": {").map(|(key, _)| key))
            .collect();
        assert_eq!(keys, BIOMES);
    }

    #[test]
    fn the_first_instance_keeps_the_face_it_always_had() {
        assert_eq!(pick(&[], "create"), "snowy");
    }

    #[test]
    fn eight_instances_get_eight_biomes_before_any_repeats() {
        let mut existing = Vec::new();
        for index in 0..BIOMES.len() {
            let id = format!("inst{index}");
            let biome = pick(&existing, &id);
            existing.push(profile(&id, Some(&biome)));
        }
        let mut seen: Vec<_> = existing.iter().map(|p| p.cover.biome.clone()).collect();
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), BIOMES.len());

        // 第九个只能重复，但不会挑一个已经用了两次的。
        existing.push(profile("extra", Some("ocean")));
        assert_ne!(pick(&existing, "ninth"), "ocean");
    }

    #[test]
    fn instances_without_a_stored_biome_count_by_their_hash() {
        // 两个老实例都按哈希落在下界上。
        let existing = [
            profile("k3v9q2mz", None),
            profile("nether2", Some("nether")),
        ];
        assert_ne!(pick(&existing, "k3v9q2mz"), "nether");
    }
}
