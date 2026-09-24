<?php
function t(int $a): void { match ($a) { 1 => 'one', default => 'other' }; }
