<?php
            class Animal2 {}
            class Handler2 { public function handle(Animal2 $a): void {} }
            class WideningHandler2 extends Handler2 { public function handle(object $a): void {} }
        