<?php
            final class Foo {
                public function fail(): void {
                    throw new \Exception("no");
                }
            }
            $f = new Foo();
            $f->fail();
        