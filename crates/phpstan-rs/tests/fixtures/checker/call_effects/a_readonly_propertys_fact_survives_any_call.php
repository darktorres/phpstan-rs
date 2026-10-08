<?php
namespace CallEffects\ReadonlyFacts;

use function PHPStan\Testing\assertType;

final readonly class Dto
{
    public function __construct(public ?string $name, public ?int $id)
    {
    }
}

final class Plain
{
    public function __construct(public readonly ?string $name, public ?int $id)
    {
    }
}

final class Documented
{
    /** @readonly */
    public ?string $name;

    public function __construct(?string $name)
    {
        $this->name = $name;
    }
}

function aReadonlyClassKeepsItsFacts(Dto $dto, callable $callback): void
{
    if ($dto->name !== null) {
        $callback();
        assertType('string', $dto->name);
    }
}

function aReadonlyPropertyKeepsItsFact(Plain $plain, callable $callback): void
{
    if ($plain->name !== null) {
        $callback();
        assertType('string', $plain->name);
    }
}

function aMutablePropertyIsForgotten(Plain $plain, callable $callback): void
{
    if ($plain->id !== null) {
        $callback();
        assertType('int|null', $plain->id);
    }
}

function aDocumentedReadonlyIsNotEnforced(Documented $documented, callable $callback): void
{
    if ($documented->name !== null) {
        $callback();
        assertType('string|null', $documented->name);
    }
}
