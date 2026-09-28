<?php namespace App;
final class Ext {
    public int $size {
        get => extension_loaded('mbstring') ? mb_strlen('x') : 0;
    }
}
