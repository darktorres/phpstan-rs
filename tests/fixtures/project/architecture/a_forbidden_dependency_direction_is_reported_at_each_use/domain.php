<?php
namespace App\Domain;
use App\Infra\Db;
final class User {
    public function __construct(private Db $db) {}
    public function save(): \App\Infra\Mailer { return new \App\Infra\Mailer(); }
    /** @return list<\App\Infra\Cache> */
    public function caches(): array { return []; }
    public function now(): \DateTimeImmutable { return new \DateTimeImmutable(); }
}
