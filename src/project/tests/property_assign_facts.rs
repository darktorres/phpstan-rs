//! `Checker::check_too_wide_property_types` (`property.unusedType`/
//! `staticProperty.unusedType`) and the two-pass collect-then-consume
//! architecture (`Project::collect_property_assign_facts`/`check_file_
//! with_property_assign_facts`) that lets a trait-donated property write
//! discovered in one file reach it for a host declared in another.

use std::sync::Arc;

use super::super::*;

#[test]
fn a_private_property_never_assigned_one_declared_union_member_is_flagged() {
    let mut project = Project::new();
    project
        .load_file(
            PathBuf::from("a.php"),
            r#"<?php
                class Foo {
                    /** @var int|string */
                    private $x;
                    public function set(): void {
                        $this->x = 1;
                    }
                }
            "#,
        )
        .unwrap();
    project.finish_loading();

    let facts = Arc::new(project.collect_property_assign_facts());
    let diags = project.check_file_with_property_assign_facts(&PathBuf::from("a.php"), &facts);
    assert!(
        diags.iter().any(|d| d.identifier == "property.unusedType"),
        "got: {diags:?}"
    );
}

#[test]
fn a_private_property_assigned_every_declared_union_member_is_never_flagged() {
    let mut project = Project::new();
    project
        .load_file(
            PathBuf::from("a.php"),
            r#"<?php
                class Foo {
                    /** @var int|string */
                    private $x;
                    public function set(): void {
                        $this->x = 1;
                        $this->x = "a";
                    }
                }
            "#,
        )
        .unwrap();
    project.finish_loading();

    let facts = Arc::new(project.collect_property_assign_facts());
    let diags = project.check_file_with_property_assign_facts(&PathBuf::from("a.php"), &facts);
    assert!(
        !diags.iter().any(|d| d.identifier == "property.unusedType"),
        "got: {diags:?}"
    );
}

#[test]
fn a_trait_donated_property_write_discovered_in_a_different_file_reaches_the_host() {
    let mut project = Project::new();
    project
        .load_file(
            PathBuf::from("trait.php"),
            r#"<?php
                trait T {
                    public function set(): void {
                        $this->x = 1;
                        $this->x = "a";
                    }
                }
            "#,
        )
        .unwrap();
    project
        .load_file(
            PathBuf::from("host.php"),
            r#"<?php
                class Foo {
                    use T;
                    /** @var int|string */
                    private $x;
                }
            "#,
        )
        .unwrap();
    project.finish_loading();

    let facts = Arc::new(project.collect_property_assign_facts());
    // The trait's own contribution was discovered while checking
    // trait.php, attributed to Foo's own fqn - host.php's own check must
    // see it too, via the whole-project merge, not just its own file's
    // live accumulation (host.php declares no method of its own that
    // assigns $x at all).
    let diags = project.check_file_with_property_assign_facts(&PathBuf::from("host.php"), &facts);
    assert!(
        !diags.iter().any(|d| d.identifier == "property.unusedType"),
        "got: {diags:?}"
    );
}

#[test]
fn a_trait_donated_property_write_missing_one_union_member_is_flagged_on_the_host() {
    let mut project = Project::new();
    project
        .load_file(
            PathBuf::from("trait.php"),
            r#"<?php
                trait T {
                    public function set(): void {
                        $this->x = 1;
                    }
                }
            "#,
        )
        .unwrap();
    project
        .load_file(
            PathBuf::from("host.php"),
            r#"<?php
                class Foo {
                    use T;
                    /** @var int|string */
                    private $x;
                }
            "#,
        )
        .unwrap();
    project.finish_loading();

    let facts = Arc::new(project.collect_property_assign_facts());
    let diags = project.check_file_with_property_assign_facts(&PathBuf::from("host.php"), &facts);
    assert!(
        diags.iter().any(|d| d.identifier == "property.unusedType"),
        "got: {diags:?}"
    );
}
