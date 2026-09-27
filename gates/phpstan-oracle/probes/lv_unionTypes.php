<?php
function u1(int $i): void {} function u2(int|string $v): void { u1($v); }
