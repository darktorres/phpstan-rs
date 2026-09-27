<?php
class PO { /** @phpstan-pure */ public function f(): int { return 1; } } class PC extends PO { public function f(): int { echo 1; return 1; } }
