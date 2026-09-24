<?php
        namespace PHPStan\FakeNs;
        /** @api */
        class Widget {
            public function getType() {}
        }
        namespace App;
        class SomeRule {
            public function __construct(private \PHPStan\FakeNs\Widget $scope) {}
            public function processNode() {
                $this->scope->getType();
            }
        }
        