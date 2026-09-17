//! DQL-aware Doctrine ORM result-type inference (PHASE_BACKLOG Phase 168,
//! 168b-168j) - end-to-end `assertType()` coverage through a minimal
//! Doctrine class hierarchy declared directly in each fixture (this
//! project vendors no real `doctrine-orm`/`doctrine-doctrine` stubs).

use super::helpers::*;

const STUBS: &str = r#"
    namespace Doctrine\ORM\Mapping {
        #[\Attribute]
        class Entity {
            public function __construct(public ?string $repositoryClass = null) {}
        }
        #[\Attribute]
        class Id {}
        #[\Attribute]
        class Column {}
        #[\Attribute]
        class ManyToOne {}
        #[\Attribute]
        class OneToMany {
            public function __construct(public string $mappedBy, public ?string $targetEntity = null) {}
        }
        #[\Attribute]
        class DiscriminatorMap {
            /** @param array<string, class-string> $value */
            public function __construct(public array $value) {}
        }
    }
    namespace Doctrine\ORM {
        class QueryBuilder {
            public function select(string ...$select): static {}
            public function addSelect(string ...$select): static {}
            public function from(string $from, string $alias): static {}
            public function join(string $join, string $alias): static {}
            public function innerJoin(string $join, string $alias): static {}
            public function leftJoin(string $join, string $alias): static {}
            public function where(string $where): static {}
            public function andWhere(string $where): static {}
            public function groupBy(string $groupBy): static {}
            public function delete(?string $delete = null, ?string $alias = null): static {}
            public function update(?string $update = null, ?string $alias = null): static {}
            public function getQuery(): Query {}
        }
        abstract class AbstractQuery {
            const HYDRATE_OBJECT = 1;
            const HYDRATE_ARRAY = 2;
            public function getResult(): mixed {}
            public function execute($parameters = null, $hydrationMode = null): mixed {}
            public function toIterable($parameters = [], $hydrationMode = null): iterable {}
            public function getOneOrNullResult($hydrationMode = null): mixed {}
            public function getSingleResult($hydrationMode = null): mixed {}
            public function getArrayResult(): array {}
            public function getSingleColumnResult(): array {}
            public function getSingleScalarResult(): mixed {}
            public function getScalarResult(): array {}
        }
        class Query extends AbstractQuery {}
        interface EntityManagerInterface {
            public function createQueryBuilder(): QueryBuilder;
            public function createQuery(string $dql): Query;
        }
        class EntityRepository {
            public function createQueryBuilder(string $alias): QueryBuilder {}
            public function getEntityManager(): EntityManagerInterface {}
        }
    }
"#;

fn diagnostics_with_stubs(src: &str) -> Vec<crate::checker::Diagnostic> {
    diagnostics_for(&format!("<?php\n{STUBS}\n{src}"))
}

fn diagnostics_with_stubs_and_config(
    src: &str,
    configure: impl FnOnce(&mut crate::registry::Registry),
) -> Vec<crate::checker::Diagnostic> {
    diagnostics_for_with(&format!("<?php\n{STUBS}\n{src}"), configure)
}

#[test]
fn get_result_via_the_explicit_entity_manager_form_is_a_list_of_the_entity() {
    let d = diagnostics_with_stubs(
        r#"
        namespace App;

        use function PHPStan\Testing\assertType;
        use Doctrine\ORM\Mapping as ORM;
        use Doctrine\ORM\EntityManagerInterface;

        #[ORM\Entity]
        class TagEntity
        {
            #[ORM\Id]
            public int $id;
        }

        class TagService
        {
            public function all(EntityManagerInterface $em): void
            {
                $rows = $em->createQueryBuilder()
                    ->select('t')
                    ->from(TagEntity::class, 't')
                    ->getQuery()
                    ->getResult();
                assertType('list<App\TagEntity>', $rows);
            }
        }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn get_result_via_the_repository_shorthand_is_a_list_of_the_entity() {
    let d = diagnostics_with_stubs(
        r#"
        namespace App;

        use function PHPStan\Testing\assertType;
        use Doctrine\ORM\Mapping as ORM;
        use Doctrine\ORM\EntityRepository;

        #[ORM\Entity(repositoryClass: TagRepository::class)]
        class TagEntity
        {
            #[ORM\Id]
            public int $id;
        }

        class TagRepository extends EntityRepository
        {
            public function all(): void
            {
                $rows = $this->createQueryBuilder('t')->getQuery()->getResult();
                assertType('list<App\TagEntity>', $rows);
            }
        }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn one_or_null_result_of_an_entity_plus_aggregate_select_is_the_mixed_shape_or_null() {
    let d = diagnostics_with_stubs(
        r#"
        namespace App;

        use function PHPStan\Testing\assertType;
        use Doctrine\ORM\Mapping as ORM;
        use Doctrine\ORM\EntityRepository;

        #[ORM\Entity(repositoryClass: TagRepository::class)]
        class TagEntity
        {
            #[ORM\Id]
            public int $id;
        }

        class TagRepository extends EntityRepository
        {
            public function countAll(): void
            {
                $row = $this->createQueryBuilder('t')
                    ->select('t', 'COUNT(t.id) AS cnt')
                    ->getQuery()
                    ->getOneOrNullResult();
                assertType('array{0: App\TagEntity, cnt: int<0, max>}|null', $row);
            }
        }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn get_single_result_of_a_single_scalar_field_select_is_still_row_shaped() {
    let d = diagnostics_with_stubs(
        r#"
        namespace App;

        use function PHPStan\Testing\assertType;
        use Doctrine\ORM\Mapping as ORM;
        use Doctrine\ORM\EntityRepository;

        #[ORM\Entity(repositoryClass: TagRepository::class)]
        class TagEntity
        {
            #[ORM\Id]
            public int $id;

            public function __construct(
                #[ORM\Column]
                public string $name,
            ) {}
        }

        class TagRepository extends EntityRepository
        {
            public function oneName(): void
            {
                $name = $this->createQueryBuilder('t')
                    ->select('t.name')
                    ->getQuery()
                    ->getSingleResult();
                assertType('array{name: string}', $name);
            }
        }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn bare_execute_behaves_identically_to_get_result() {
    let d = diagnostics_with_stubs(
        r#"
        namespace App;

        use function PHPStan\Testing\assertType;
        use Doctrine\ORM\Mapping as ORM;
        use Doctrine\ORM\EntityRepository;

        #[ORM\Entity(repositoryClass: TagRepository::class)]
        class TagEntity
        {
            #[ORM\Id]
            public int $id;
        }

        class TagRepository extends EntityRepository
        {
            public function all(): void
            {
                $rows = $this->createQueryBuilder('t')->getQuery()->execute();
                assertType('list<App\TagEntity>', $rows);
            }
        }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_repository_class_shared_by_two_entities_is_undecidable() {
    // Real, valid Doctrine: one repository *class* can be instantiated
    // separately per entity via `EntityManager::getRepository()`/DI (a
    // real Piwigo shape - `ImageRepository` serves both `ImageEntity`
    // and `ImageFormatEntity`) - nothing in the repository class's own
    // method bodies says which entity a given call site's instance is
    // actually bound to, so guessing either one would be a real false
    // positive/negative risk, not a safe simplification.
    let d = diagnostics_with_stubs(
        r#"
        namespace App;

        use function PHPStan\Testing\assertType;
        use Doctrine\ORM\Mapping as ORM;
        use Doctrine\ORM\EntityRepository;

        #[ORM\Entity(repositoryClass: TagRepository::class)]
        class TagEntity
        {
            #[ORM\Id]
            public int $id;
        }

        #[ORM\Entity(repositoryClass: TagRepository::class)]
        class TagFormatEntity
        {
            #[ORM\Id]
            public int $id;
        }

        class TagRepository extends EntityRepository
        {
            public function all(): void
            {
                $rows = $this->createQueryBuilder('t')->getQuery()->getResult();
                assertType('list', $rows);
            }
        }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_delete_chains_execute_is_a_plain_affected_row_count() {
    // `EntityRepository::createQueryBuilder($alias)` always pre-populates
    // its own `select($alias)`/`from(...)` (real Doctrine sugar), and
    // `->delete()` only flips an internal query-type flag without
    // clearing that select - so this chain looks exactly like an
    // ordinary `SELECT t FROM TagEntity t` to everything but the `dml`
    // flag. Real `AbstractQuery::_doExecute()` returns a plain int for
    // any DML statement, bypassing hydration entirely - the real Piwigo
    // pattern this targets (`*Repository::purgeOlderThan(): int`).
    let d = diagnostics_with_stubs(
        r#"
        namespace App;

        use function PHPStan\Testing\assertType;
        use Doctrine\ORM\Mapping as ORM;
        use Doctrine\ORM\EntityRepository;

        #[ORM\Entity(repositoryClass: TagRepository::class)]
        class TagEntity
        {
            #[ORM\Id]
            public int $id;
        }

        class TagRepository extends EntityRepository
        {
            public function purgeAll(): int
            {
                return $this->createQueryBuilder('t')
                    ->delete()
                    ->getQuery()
                    ->execute();
            }
        }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn execute_with_an_explicit_non_object_hydration_mode_falls_back_to_mixed() {
    let d = diagnostics_with_stubs(
        r#"
        namespace App;

        use function PHPStan\Testing\assertType;
        use Doctrine\ORM\Mapping as ORM;
        use Doctrine\ORM\EntityRepository;
        use Doctrine\ORM\AbstractQuery;

        #[ORM\Entity(repositoryClass: TagRepository::class)]
        class TagEntity
        {
            #[ORM\Id]
            public int $id;
        }

        class TagRepository extends EntityRepository
        {
            public function all(): void
            {
                $rows = $this->createQueryBuilder('t')->getQuery()->execute(null, AbstractQuery::HYDRATE_ARRAY);
                assertType('mixed', $rows);
            }
        }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn an_undecidable_chain_falls_back_to_todays_shape_only_list_for_get_result() {
    let d = diagnostics_with_stubs(
        r#"
        namespace App;

        use function PHPStan\Testing\assertType;
        use Doctrine\ORM\EntityManagerInterface;

        class TagService
        {
            public function all(EntityManagerInterface $em, string $dql): void
            {
                $rows = $em->createQueryBuilder()->select($dql)->getQuery()->getResult();
                assertType('list', $rows);
            }
        }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn get_array_result_converts_the_entity_into_an_object_shape() {
    let d = diagnostics_with_stubs(
        r#"
        namespace App;

        use function PHPStan\Testing\assertType;
        use Doctrine\ORM\Mapping as ORM;
        use Doctrine\ORM\EntityRepository;

        #[ORM\Entity(repositoryClass: TagRepository::class)]
        class TagEntity
        {
            #[ORM\Id]
            public int $id;

            public function __construct(
                #[ORM\Column]
                public string $name,
            ) {}
        }

        class TagRepository extends EntityRepository
        {
            public function all(): void
            {
                $rows = $this->createQueryBuilder('t')->getQuery()->getArrayResult();
                assertType('list<object{id: int, name: string}>', $rows);
            }
        }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn get_single_column_result_of_one_scalar_field_is_a_list_of_that_type() {
    let d = diagnostics_with_stubs(
        r#"
        namespace App;

        use function PHPStan\Testing\assertType;
        use Doctrine\ORM\Mapping as ORM;
        use Doctrine\ORM\EntityRepository;

        #[ORM\Entity(repositoryClass: TagRepository::class)]
        class TagEntity
        {
            #[ORM\Id]
            public int $id;

            public function __construct(
                #[ORM\Column]
                public string $name,
            ) {}
        }

        class TagRepository extends EntityRepository
        {
            public function allNames(): void
            {
                $names = $this->createQueryBuilder('t')
                    ->select('t.name')
                    ->getQuery()
                    ->getSingleColumnResult();
                assertType('list<string>', $names);
            }
        }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn get_single_column_result_of_a_value_object_id_field_falls_back_to_mixed() {
    // Unlike `getSingleScalarResult()`'s own `SingleScalarHydrator`,
    // `getSingleColumnResult()`'s `ScalarColumnHydrator` does a raw
    // `fetchFirstColumn()` with no type conversion at all - a
    // custom-`Type`-mapped id field (a real Piwigo pattern: `?GroupId
    // $id = null` backed by `#[ORM\Column(type: 'group_id')]`) never
    // actually comes back as that value object here.
    let d = diagnostics_with_stubs(
        r#"
        namespace App;

        use function PHPStan\Testing\assertType;
        use Doctrine\ORM\Mapping as ORM;
        use Doctrine\ORM\EntityRepository;

        final class TagId
        {
            public function __construct(public readonly int $value) {}
        }

        #[ORM\Entity(repositoryClass: TagRepository::class)]
        class TagEntity
        {
            #[ORM\Id]
            #[ORM\Column]
            public ?TagId $id = null;

            public function __construct(
                #[ORM\Column]
                public string $name,
            ) {}
        }

        class TagRepository extends EntityRepository
        {
            public function allIds(): void
            {
                $ids = $this->createQueryBuilder('t')
                    ->select('t.id')
                    ->getQuery()
                    ->getSingleColumnResult();
                assertType('list', $ids);
            }
        }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn get_single_scalar_result_of_one_scalar_field_is_that_fields_own_type() {
    let d = diagnostics_with_stubs(
        r#"
        namespace App;

        use function PHPStan\Testing\assertType;
        use Doctrine\ORM\Mapping as ORM;
        use Doctrine\ORM\EntityRepository;

        #[ORM\Entity(repositoryClass: TagRepository::class)]
        class TagEntity
        {
            #[ORM\Id]
            public int $id;

            public function __construct(
                #[ORM\Column]
                public string $name,
            ) {}
        }

        class TagRepository extends EntityRepository
        {
            public function oneName(): void
            {
                $name = $this->createQueryBuilder('t')
                    ->select('t.name')
                    ->getQuery()
                    ->getSingleScalarResult();
                assertType('string', $name);
            }
        }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn get_scalar_result_flattens_the_entity_under_alias_prefixed_keys() {
    let d = diagnostics_with_stubs(
        r#"
        namespace App;

        use function PHPStan\Testing\assertType;
        use Doctrine\ORM\Mapping as ORM;
        use Doctrine\ORM\EntityRepository;

        #[ORM\Entity(repositoryClass: TagRepository::class)]
        class TagEntity
        {
            #[ORM\Id]
            public int $id;

            public function __construct(
                #[ORM\Column]
                public string $name,
            ) {}
        }

        class TagRepository extends EntityRepository
        {
            public function all(): void
            {
                $rows = $this->createQueryBuilder('t')->getQuery()->getScalarResult();
                assertType('list<array{t_id: int, t_name: string}>', $rows);
            }
        }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn create_query_with_a_literal_dql_string_is_a_list_of_the_entity() {
    let d = diagnostics_with_stubs(
        r#"
        namespace App;

        use function PHPStan\Testing\assertType;
        use Doctrine\ORM\Mapping as ORM;
        use Doctrine\ORM\EntityManagerInterface;

        #[ORM\Entity]
        class TagEntity
        {
            #[ORM\Id]
            public int $id;
        }

        class TagService
        {
            public function all(EntityManagerInterface $em): void
            {
                $rows = $em->createQuery('SELECT t FROM App\TagEntity t')->getResult();
                assertType('list<App\TagEntity>', $rows);
            }
        }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn create_query_with_a_non_literal_dql_string_falls_back_to_todays_shape_only_list() {
    let d = diagnostics_with_stubs(
        r#"
        namespace App;

        use function PHPStan\Testing\assertType;
        use Doctrine\ORM\EntityManagerInterface;

        class TagService
        {
            public function all(EntityManagerInterface $em, string $dql): void
            {
                $rows = $em->createQuery($dql)->getResult();
                assertType('list', $rows);
            }
        }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_declared_custom_dql_function_resolves_to_its_own_config_category() {
    let d = diagnostics_with_stubs_and_config(
        r#"
        namespace App;

        use function PHPStan\Testing\assertType;
        use Doctrine\ORM\Mapping as ORM;
        use Doctrine\ORM\EntityRepository;

        #[ORM\Entity(repositoryClass: TagRepository::class)]
        class TagEntity
        {
            #[ORM\Id]
            public int $id;

            public function __construct(
                #[ORM\Column]
                public string $name,
            ) {}
        }

        class TagRepository extends EntityRepository
        {
            public function months(): void
            {
                $rows = $this->createQueryBuilder('t')
                    ->select('t.id', 'DATE_FORMAT_YEAR_MONTH(t.name) AS ym')
                    ->getQuery()
                    ->getResult();
                assertType('list<array{id: int, ym: string}>', $rows);
            }
        }
        "#,
        |r| {
            r.doctrine_custom_functions.insert(
                "date_format_year_month".to_string(),
                crate::doctrine_functions::DoctrineFunctionKind::String,
            );
        },
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_polymorphic_from_infers_a_union_across_every_mapped_subclass() {
    let d = diagnostics_with_stubs(
        r#"
        namespace App;

        use function PHPStan\Testing\assertType;
        use Doctrine\ORM\Mapping as ORM;
        use Doctrine\ORM\EntityManagerInterface;

        #[ORM\Entity]
        class CatEntity
        {
            #[ORM\Id]
            public int $id;
        }
        #[ORM\Entity]
        class DogEntity
        {
            #[ORM\Id]
            public int $id;
        }

        #[ORM\Entity]
        #[ORM\DiscriminatorMap(['cat' => CatEntity::class, 'dog' => DogEntity::class])]
        class AnimalEntity
        {
            #[ORM\Id]
            public int $id;
        }

        class AnimalService
        {
            public function all(EntityManagerInterface $em): void
            {
                $rows = $em->createQueryBuilder()
                    ->select('a')
                    ->from(AnimalEntity::class, 'a')
                    ->getQuery()
                    ->getResult();
                assertType('list<App\CatEntity|App\DogEntity>', $rows);
            }
        }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}

#[test]
fn a_scalar_subquery_select_item_resolves_its_own_single_column_type() {
    let d = diagnostics_with_stubs(
        r#"
        namespace App;

        use function PHPStan\Testing\assertType;
        use Doctrine\ORM\Mapping as ORM;
        use Doctrine\ORM\EntityManagerInterface;

        #[ORM\Entity]
        class TagEntity
        {
            #[ORM\Id]
            public int $id;
        }
        #[ORM\Entity]
        class ImageTagEntity
        {
            #[ORM\Id]
            public int $imageId;
        }

        class TagService
        {
            public function all(EntityManagerInterface $em): void
            {
                $rows = $em->createQuery(
                    'SELECT t, (SELECT it.imageId FROM App\ImageTagEntity it) AS firstImageId FROM App\TagEntity t'
                )->getResult();
                assertType('list<array{0: App\TagEntity, firstImageId: int}>', $rows);
            }
        }
        "#,
    );
    assert!(d.is_empty(), "unexpected diagnostics: {d:?}");
}
