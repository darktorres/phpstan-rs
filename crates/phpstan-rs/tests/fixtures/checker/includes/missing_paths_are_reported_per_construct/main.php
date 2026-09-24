<?php
require __DIR__ . '/lib/absent.php';
require_once __DIR__ . '/nope.php';
include 'missing.php';
include_once dirname(__DIR__, 1) . '/elsewhere.php';
