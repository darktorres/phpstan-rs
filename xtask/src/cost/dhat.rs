//! Reading valgrind's DHAT output (`--tool=dhat`, file version 2).

use serde_json::Value;

/// Heap totals and the allocation sites by bytes, largest first.
pub(crate) struct Heap {
    pub(crate) blocks: u64,
    pub(crate) bytes: u64,
    /// Bytes live at the global peak (`t-gmax`).
    pub(crate) peak: u64,
    /// Bytes still live when the program ended (`t-end`).
    pub(crate) live: u64,
    pub(crate) sites: Vec<(String, u64)>,
}

/// Frames that only say an allocation happened: the allocator's entry
/// points, and anything inside Rust's `alloc` and `core` libraries. A site
/// is named by the first frames after them.
const ALLOCATOR_NAMES: [&str; 6] = [
    "malloc", "calloc", "realloc", "__rust_", "alloc::", "<alloc::",
];
/// Rust's library frames read `alloc/src/...` and `core/src/...`, as the
/// meter's `--fullpath-after` options spell them.
const ALLOCATOR_FILES: [&str; 2] = ["(alloc/src/", "(core/src/"];

fn is_allocator(frame: &str) -> bool {
    ALLOCATOR_NAMES.iter().any(|name| frame.starts_with(name))
        || ALLOCATOR_FILES.iter().any(|file| frame.contains(file))
}

/// Frames shown per site.
const SITE_FRAMES: usize = 3;

pub(crate) fn parse(text: &str) -> Result<Heap, String> {
    let json: Value =
        serde_json::from_str(text).map_err(|error| format!("DHAT output: {error}"))?;
    let frames: Vec<&str> = json["ftbl"]
        .as_array()
        .ok_or("DHAT output has no ftbl")?
        .iter()
        .map(|frame| frame.as_str().unwrap_or_default())
        .collect();
    let mut heap = Heap {
        blocks: 0,
        bytes: 0,
        peak: 0,
        live: 0,
        sites: Vec::new(),
    };
    let mut sites = std::collections::BTreeMap::<String, u64>::new();
    for point in json["pps"].as_array().ok_or("DHAT output has no pps")? {
        let count = |key: &str| point[key].as_u64().unwrap_or(0);
        heap.blocks += count("tbk");
        heap.bytes += count("tb");
        heap.peak += count("gb");
        heap.live += count("eb");
        let stack: Vec<&str> = point["fs"]
            .as_array()
            .map(|fs| {
                fs.iter()
                    .filter_map(Value::as_u64)
                    .filter_map(|i| frames.get(usize::try_from(i).ok()?).copied())
                    .map(function)
                    .collect()
            })
            .unwrap_or_default();
        let start = stack
            .iter()
            .position(|frame| !is_allocator(frame))
            .unwrap_or(0);
        let site = stack[start..]
            .iter()
            .take(SITE_FRAMES)
            .copied()
            .collect::<Vec<_>>()
            .join(" < ");
        *sites.entry(site).or_default() += count("tb");
    }
    heap.sites = sites.into_iter().collect();
    heap.sites
        .sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    Ok(heap)
}

/// `0x4A1B2C: name (file:line)` without its address, which moves between
/// builds.
fn function(frame: &str) -> &str {
    frame.split_once(": ").map_or(frame, |(_, rest)| rest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn totals_sum_every_program_point_and_sites_skip_the_allocator() {
        let text = r#"{"ftbl":["[root]","0x1: malloc (a.c:1)","0x2: alloc (alloc/src/alloc.rs:130)","0x3: parse (p.rs:3)","0x4: main (m.rs:4)"],
            "pps":[{"tb":10,"tbk":1,"gb":10,"eb":0,"fs":[1,2,3,4]},{"tb":5,"tbk":2,"gb":0,"eb":5,"fs":[1,4]}]}"#;
        let heap = parse(text).unwrap();
        assert_eq!(
            (heap.blocks, heap.bytes, heap.peak, heap.live),
            (3, 15, 10, 5)
        );
        assert_eq!(
            heap.sites[0],
            ("parse (p.rs:3) < main (m.rs:4)".to_owned(), 10)
        );
        assert_eq!(heap.sites[1], ("main (m.rs:4)".to_owned(), 5));
    }
}
