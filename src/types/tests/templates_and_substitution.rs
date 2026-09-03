//! Template type substitution.
use super::super::*;

#[test]
fn substitute_template_recurses_into_array_and_intersection_types() {
    // Coverage measurement found substitute_template's Array and
    // Intersection arms had zero executions - every existing
    // template-substitution test only substitutes a bare
    // TemplateParam or one nested inside a Union, never `T[]`/
    // `array<T>` or an intersection type mentioning T.
    let mut bindings = HashMap::new();
    bindings.insert("T".to_string(), Type::String);

    let array_of_t = Type::Array(Box::new(Type::TemplateParam("T".to_string())));
    assert_eq!(
        substitute_template(&array_of_t, &bindings),
        Type::Array(Box::new(Type::String))
    );

    let intersection_with_t = Type::Intersection(vec![
        Type::TemplateParam("T".to_string()),
        Type::Object("Countable".to_string(), Vec::new()),
    ]);
    assert_eq!(
        substitute_template(&intersection_with_t, &bindings),
        Type::Intersection(vec![
            Type::String,
            Type::Object("Countable".to_string(), Vec::new())
        ])
    );
}
