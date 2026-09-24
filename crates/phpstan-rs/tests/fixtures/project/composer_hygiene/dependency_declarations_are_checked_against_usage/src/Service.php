<?php namespace App;
use Acme\Used\Used;
use Acme\Transitive\Helper;
use Acme\DevTool\Tool;
final class Service {
    public function run(): void {
        new Used();
        new Helper();
        \Acme\Transitive\helper();
        new Tool();
    }
}
