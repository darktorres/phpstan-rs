<?php
            function a(): void {}
            function b(): void {}
            function g(): int { return 1; }
            function f(int $x): void {
                $r = match ($x) { 1 => a(), default => b() };
                match ($x) { 1 => a(), default => b() };
                $s = match ($x) { 1 => a(), default => throw new \Exception() };
                $t = match ($x) { 1 => a(), default => g() };
            }
        