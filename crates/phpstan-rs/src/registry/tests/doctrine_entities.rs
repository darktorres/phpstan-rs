//! Doctrine ORM entity metadata extraction (ROADMAP Phase 168 168a).

use super::super::*;
use super::helpers::*;

#[test]
fn an_ordinary_class_has_no_doctrine_entity() {
    let r = build_registry(
        r#"<?php
            class Foo {}
            "#,
    );
    assert!(r.classes.get("Foo").unwrap().doctrine_entity.is_none());
}

#[test]
fn an_entity_records_its_id_field_scalar_fields_and_repository_class() {
    let r = build_registry(
        r#"<?php
            namespace App;
            use Doctrine\ORM\Mapping as ORM;

            class TagRepository {}

            #[ORM\Entity(repositoryClass: TagRepository::class)]
            class TagEntity
            {
                #[ORM\Id]
                #[ORM\Column]
                public int $id;

                public function __construct(
                    #[ORM\Column]
                    public string $name,
                ) {}
            }
            "#,
    );
    let entity = r
        .classes
        .get("App\\TagEntity")
        .unwrap()
        .doctrine_entity
        .as_ref()
        .unwrap();
    assert_eq!(entity.id_field.as_deref(), Some("id"));
    assert_eq!(entity.fields.get("id"), Some(&Type::Int));
    assert_eq!(entity.fields.get("name"), Some(&Type::String));
    assert_eq!(
        entity.repository_class.as_deref(),
        Some("App\\TagRepository")
    );
}

#[test]
fn a_table_attribute_with_an_explicit_name_is_recorded() {
    let r = build_registry(
        r#"<?php
            namespace App;
            use Doctrine\ORM\Mapping as ORM;

            #[ORM\Entity]
            #[ORM\Table(name: 'tags')]
            class TagEntity
            {
                #[ORM\Id]
                #[ORM\Column]
                public int $id;
            }
            "#,
    );
    let entity = r
        .classes
        .get("App\\TagEntity")
        .unwrap()
        .doctrine_entity
        .as_ref()
        .unwrap();
    assert_eq!(entity.table_name.as_deref(), Some("tags"));
}

#[test]
fn no_table_attribute_at_all_leaves_table_name_none() {
    let r = build_registry(
        r#"<?php
            namespace App;
            use Doctrine\ORM\Mapping as ORM;

            #[ORM\Entity]
            class TagEntity
            {
                #[ORM\Id]
                #[ORM\Column]
                public int $id;
            }
            "#,
    );
    let entity = r
        .classes
        .get("App\\TagEntity")
        .unwrap()
        .doctrine_entity
        .as_ref()
        .unwrap();
    assert!(entity.table_name.is_none());
}

#[test]
fn a_column_name_argument_overrides_the_property_name() {
    let r = build_registry(
        r#"<?php
            namespace App;
            use Doctrine\ORM\Mapping as ORM;

            #[ORM\Entity]
            #[ORM\Table(name: 'tags')]
            class TagEntity
            {
                #[ORM\Id]
                #[ORM\Column]
                public int $id;

                public function __construct(
                    #[ORM\Column(name: 'display_name')]
                    public string $name,
                ) {}
            }
            "#,
    );
    let entity = r
        .classes
        .get("App\\TagEntity")
        .unwrap()
        .doctrine_entity
        .as_ref()
        .unwrap();
    assert_eq!(entity.column_names.get("id").map(|s| &**s), Some("id"));
    assert_eq!(
        entity.column_names.get("name").map(|s| &**s),
        Some("display_name")
    );
}

#[test]
fn a_many_to_one_association_resolves_its_target_from_the_native_type() {
    let r = build_registry(
        r#"<?php
            namespace App;
            use Doctrine\ORM\Mapping as ORM;

            #[ORM\Entity]
            class CategoryEntity {}

            #[ORM\Entity]
            class ImageEntity
            {
                #[ORM\Id]
                public int $id;

                public function __construct(
                    #[ORM\ManyToOne]
                    public CategoryEntity $category,
                ) {}
            }
            "#,
    );
    let entity = r
        .classes
        .get("App\\ImageEntity")
        .unwrap()
        .doctrine_entity
        .as_ref()
        .unwrap();
    let assoc = entity.associations.get("category").unwrap();
    assert_eq!(assoc.target, "App\\CategoryEntity".into());
    assert!(!assoc.is_collection);
}

#[test]
fn a_one_to_many_collection_association_resolves_its_target_from_target_entity_argument() {
    let r = build_registry(
        r#"<?php
            namespace App;
            use Doctrine\ORM\Mapping as ORM;

            #[ORM\Entity]
            class ImageCategoryEntity {}

            #[ORM\Entity]
            class CategoryEntity
            {
                #[ORM\Id]
                public int $id;

                #[ORM\OneToMany(mappedBy: 'category', targetEntity: ImageCategoryEntity::class)]
                public array $imageCategories;
            }
            "#,
    );
    let entity = r
        .classes
        .get("App\\CategoryEntity")
        .unwrap()
        .doctrine_entity
        .as_ref()
        .unwrap();
    let assoc = entity.associations.get("imageCategories").unwrap();
    assert_eq!(assoc.target, "App\\ImageCategoryEntity".into());
    assert!(assoc.is_collection);
}

#[test]
fn a_composite_identifier_records_no_single_id_field() {
    let r = build_registry(
        r#"<?php
            namespace App;
            use Doctrine\ORM\Mapping as ORM;

            #[ORM\Entity]
            class ImageEntity {}
            #[ORM\Entity]
            class CategoryEntity {}

            #[ORM\Entity]
            class ImageCategoryEntity
            {
                public function __construct(
                    #[ORM\Id]
                    #[ORM\ManyToOne]
                    public ImageEntity $image,
                    #[ORM\Id]
                    #[ORM\ManyToOne]
                    public CategoryEntity $category,
                ) {}
            }
            "#,
    );
    let entity = r
        .classes
        .get("App\\ImageCategoryEntity")
        .unwrap()
        .doctrine_entity
        .as_ref()
        .unwrap();
    assert!(entity.id_field.is_none());
    assert_eq!(entity.associations.len(), 2);
}

#[test]
fn a_root_entity_records_its_discriminator_map() {
    let r = build_registry(
        r#"<?php
            namespace App;
            use Doctrine\ORM\Mapping as ORM;

            #[ORM\Entity]
            class CatEntity {}
            #[ORM\Entity]
            class DogEntity {}

            #[ORM\Entity]
            #[ORM\InheritanceType('SINGLE_TABLE')]
            #[ORM\DiscriminatorColumn(name: 'type')]
            #[ORM\DiscriminatorMap(['cat' => CatEntity::class, 'dog' => DogEntity::class])]
            class AnimalEntity
            {
                #[ORM\Id]
                public int $id;
            }
            "#,
    );
    let entity = r
        .classes
        .get("App\\AnimalEntity")
        .unwrap()
        .doctrine_entity
        .as_ref()
        .unwrap();
    assert_eq!(
        entity.discriminator_map.get("cat").map(|s| s.as_ref()),
        Some("App\\CatEntity")
    );
    assert_eq!(
        entity.discriminator_map.get("dog").map(|s| s.as_ref()),
        Some("App\\DogEntity")
    );
}

#[test]
fn an_ordinary_entity_has_no_discriminator_map() {
    let r = build_registry(
        r#"<?php
            namespace App;
            use Doctrine\ORM\Mapping as ORM;

            #[ORM\Entity]
            class TagEntity
            {
                #[ORM\Id]
                public int $id;
            }
            "#,
    );
    let entity = r
        .classes
        .get("App\\TagEntity")
        .unwrap()
        .doctrine_entity
        .as_ref()
        .unwrap();
    assert!(entity.discriminator_map.is_empty());
}
