<?php
            class OldService {}
            class Foo {
                public function __construct(OldService $service) {}
                public function handle(OldService $svc): void {
                    $old = new OldService();
                }
                public function get(): OldService {}
            }
        