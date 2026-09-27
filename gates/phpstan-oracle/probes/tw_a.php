<?php
final class TF { public function m(): int|string { return 1; } }
class TP { private function p(): int|string { return 1; } public function q(): int { return $this->p(); } }
class TN { public function m(): int|string { return 1; } }
