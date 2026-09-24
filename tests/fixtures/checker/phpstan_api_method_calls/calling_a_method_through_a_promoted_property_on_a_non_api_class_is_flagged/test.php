<?php
        namespace PHPStan\FakeNs;
        class Widget {
            public function getType() {}
        }
        namespace App\Rules;
        class SomeRule {
            public function __construct(private \PHPStan\FakeNs\Widget $scope) {}
            public function processNode() {
                $this->scope->getType();
            }
        }
        