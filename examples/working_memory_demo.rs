use membrid::{
    memory::working::{OverflowStrategy, WorkingMemory},
    types::{Episode, MemoryTier, Role},
};

fn main() {
    // --- basic push + scan ---
    println!("=== basic push + scan ===");
    let mut wm = WorkingMemory::new(5);

    let turns = [
        (Role::User, "What's the weather like today?"),
        (Role::Assistant, "It's sunny and 22°C in Tokyo."),
        (Role::User, "What about tomorrow?"),
        (Role::Assistant, "Partly cloudy, around 19°C."),
    ];

    for (role, content) in turns {
        wm.push(Episode::new("demo-session", role, content));
    }

    println!("buffer size: {}/{}", wm.len(), wm.max_turns());
    for mem in wm.scan() {
        let role = mem.metadata["role"].as_str().unwrap_or("?");
        println!("  [{role}] score={:.1} tier={:?}  {:?}", mem.score, mem.tier, mem.content);
    }
    assert_eq!(wm.scan().iter().map(|m| m.tier.clone()).collect::<Vec<_>>(),
               vec![MemoryTier::Working; 4]);

    // --- overflow: DropOldest ---
    println!("\n=== overflow DropOldest (max_turns=3) ===");
    let mut wm2 = WorkingMemory::new(3);
    for i in 1..=5u32 {
        wm2.push(Episode::new("s", Role::User, format!("turn {i}")));
    }
    // only turns 3,4,5 should remain
    let contents: Vec<_> = wm2.scan().into_iter().map(|m| m.content).collect();
    println!("  remaining: {:?}", contents);
    assert_eq!(contents, ["turn 3", "turn 4", "turn 5"]);

    // --- overflow: SummarizeOldest (Phase 1 falls back to DropOldest) ---
    println!("\n=== overflow SummarizeOldest (falls back, max_turns=2) ===");
    let mut wm3 = WorkingMemory::new(2).with_overflow(OverflowStrategy::SummarizeOldest);
    wm3.push(Episode::new("s", Role::User, "a"));
    wm3.push(Episode::new("s", Role::Assistant, "b"));
    wm3.push(Episode::new("s", Role::User, "c")); // "a" dropped
    let contents: Vec<_> = wm3.scan().into_iter().map(|m| m.content).collect();
    println!("  remaining: {:?}", contents);
    assert_eq!(contents, ["b", "c"]);

    // --- clear ---
    println!("\n=== clear ===");
    wm.clear();
    println!("  after clear: len={} is_empty={}", wm.len(), wm.is_empty());
    assert!(wm.is_empty());

    println!("\nAll assertions passed.");
}
