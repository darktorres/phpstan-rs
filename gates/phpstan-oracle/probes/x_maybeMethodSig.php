<?php
class MP1 { /** @param int $x */ public function m($x): void {} } class MC1 extends MP1 { /** @param int|string $x */ public function m($x): void {} } class MP2 { /** @param int|string $x */ public function m($x): void {} } class MC2 extends MP2 { /** @param int $x */ public function m($x): void {} }
