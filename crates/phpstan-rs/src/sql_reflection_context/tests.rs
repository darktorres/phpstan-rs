use super::*;

#[test]
fn an_unconfigured_context_reflects_nothing_and_remembers_the_answer() {
    let context = SqlContext::default();
    let sql = "SELECT id FROM categories";
    assert!(context.reflect(sql).is_none());
    assert!(context.reflected.lock().unwrap().contains_key(sql));
    assert!(context.reflect(sql).is_none());
}
