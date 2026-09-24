<?php
define('ROOT', dirname(__FILE__));
class Loader {
    const LIB = ROOT . '/lib/';
    public function load(): void {
        require self::LIB . 'present.php';
        require static::LIB . 'absent.php';
    }
}
