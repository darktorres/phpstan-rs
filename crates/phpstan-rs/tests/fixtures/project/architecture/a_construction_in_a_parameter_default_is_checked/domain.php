<?php
namespace App\Domain;
use App\Infra\Db;
final class User {
    public function __construct(public readonly Db $db = new Db()) {}
}
