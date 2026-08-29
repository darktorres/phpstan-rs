<?php

namespace App\Repo;

use App\Model\User;

interface Repository
{
    public function find(int $id): ?User;
}

abstract class BaseRepository implements Repository
{
    protected array $items = [];

    public function count(): int
    {
        return count($this->items);
    }
}

class UserRepository extends BaseRepository
{
    public const TABLE = 'users';

    public function find(int $id): ?User
    {
        foreach ($this->items as $item) {
            if ($item->id === $id) {
                return $item;
            }
        }
        return null;
    }

    public static function make(): self
    {
        return new self();
    }

    public function search(string $q, int $limit = 10, ...$extra): array
    {
        $results = [];
        $i = 0;
        while ($i < $limit) {
            $results[] = $this->items[$i] ?? null;
            $i++;
        }
        return $results;
    }
}

function describe(?User $u): string
{
    try {
        return $u === null ? "none" : $u->name;
    } catch (\Throwable $e) {
        return "error: " . $e->getMessage();
    } finally {
        echo "done\n";
    }
}

$repo = UserRepository::make();
echo UserRepository::TABLE;
echo $repo->count() + 1 * 2 ** 3;
