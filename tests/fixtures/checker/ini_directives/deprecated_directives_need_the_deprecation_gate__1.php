<?php
function f(bool $b): void {
    ini_get('track_errors');
    ini_set('assert.active', '1');
    ini_get('assert.active');
    ini_restore(option: 'mysqli.reconnect');
    ini_set($b ? 'allow_url_include' : 'memory_limit', '1');
    ini_get('TRACK_ERRORS');
    ini_get('memory_limit');
}
