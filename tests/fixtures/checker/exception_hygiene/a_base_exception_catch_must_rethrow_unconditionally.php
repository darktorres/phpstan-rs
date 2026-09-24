<?php
function f(): void {
    try { g(); } catch (\Throwable $e) { log_it($e); }
    try { g(); } catch (\Exception $e) { log_it($e); throw $e; }
    try { g(); } catch (\Exception $e) { if (h()) { throw $e; } }
    try { g(); } catch (\InvalidArgumentException $e) { log_it($e); }
}
function g(): void {}
function h(): bool { return true; }
function log_it(\Throwable $e): void {}
