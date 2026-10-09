//! The method and property results of a class, against a scan of the whole
//! result tables that keeps the entries of the class.

use super::super::*;
use super::helpers::*;

fn reference_methods(registry: &Registry, class: ClassId) -> Vec<(MethodRef, usize)> {
    let mut out: Vec<(MethodRef, usize)> = registry
        .deferred_class_result(class)
        .into_iter()
        .flat_map(|results| results.methods.iter())
        .map(|(method, signature)| (*method, std::ptr::from_ref(signature) as usize))
        .collect();
    out.extend(
        registry
            .class_method_results
            .iter()
            .filter(|(method, _)| method.class() == class)
            .map(|(method, signature)| (*method, std::ptr::from_ref(signature.get()) as usize)),
    );
    out.sort();
    out
}

fn reference_properties(registry: &Registry, class: ClassId) -> Vec<(PropertyRef, usize)> {
    let mut out: Vec<(PropertyRef, usize)> = registry
        .deferred_class_result(class)
        .into_iter()
        .flat_map(|results| results.properties.iter())
        .filter_map(|(name, property)| {
            registry
                .property_ref_by_name(class, name)
                .map(|reference| (reference, Arc::as_ptr(property) as usize))
        })
        .collect();
    out.extend(
        registry
            .class_property_results
            .iter()
            .filter(|(property, _)| property.class() == class)
            .map(|(property, value)| (*property, Arc::as_ptr(value) as usize)),
    );
    out.sort();
    out
}

const SOURCE: &str = r#"<?php
    interface Named { public function name(): string; }
    trait Counts {
        public int $count = 0;
        /** @return list<int> */
        public function counted(): array { return []; }
    }
    /**
     * @method int ghost()
     * @property string $label
     */
    class Widget implements Named {
        use Counts;
        public function name(): string { return 'w'; }
    }
    class Gadget extends Widget {
        public function name(): string { return 'g'; }
    }
    enum Suit: string {
        case Hearts = 'h';
        public function label(): string { return $this->value; }
    }
"#;

#[test]
fn the_results_of_a_class_are_the_entries_the_tables_hold_for_it() {
    let registry = build_registry(SOURCE);
    let mut seen_method = false;
    let mut seen_property = false;
    for (class, _, _) in registry.classes_by_id() {
        let mut methods: Vec<(MethodRef, usize)> = registry
            .method_results_for_class(class)
            .map(|(method, signature)| (method, std::ptr::from_ref(signature) as usize))
            .collect();
        methods.sort();
        assert_eq!(methods, reference_methods(&registry, class));
        seen_method |= !methods.is_empty();
        let mut properties: Vec<(PropertyRef, usize)> = registry
            .property_results_for_class(class)
            .map(|(property, value)| (property, Arc::as_ptr(value) as usize))
            .collect();
        properties.sort();
        assert_eq!(properties, reference_properties(&registry, class));
        seen_property |= !properties.is_empty();
        let raw: Vec<MethodRef> = registry
            .raw_method_results_for_class(class)
            .map(|(method, _)| method)
            .collect();
        assert!(raw.iter().all(|method| method.class() == class));
    }
    assert!(seen_method && seen_property);
}
