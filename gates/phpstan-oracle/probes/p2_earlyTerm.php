<?php
class ET { public function stop(): void { exit; } } function et(bool $c, ET $e): int { if ($c) { $r = 1; } else { $e->stop(); } return $r; }
