<?php
const LIB = __DIR__ . '/lib';
class Paths { const FILE = 'absent.php'; }
require LIB . '/present.php';
require LIB . '/' . Paths::FILE;
