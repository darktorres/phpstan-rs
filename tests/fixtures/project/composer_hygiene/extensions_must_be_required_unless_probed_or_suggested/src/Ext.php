<?php namespace App;
final class Ext {
    public function f(): void {
        mb_strlen('x');
        mb_substr('x', 0);
        json_encode([]);
        strlen('x');
        new \Imagick();
        if (extension_loaded('gd')) { imagecreate(1, 1); }
    }
}
