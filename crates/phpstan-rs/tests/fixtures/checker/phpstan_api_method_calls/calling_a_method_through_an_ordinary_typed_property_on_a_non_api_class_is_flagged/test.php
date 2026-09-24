<?php
        namespace PHPStan\FakeNs;
        class Widget {
            public function getType() {}
        }
        namespace App\Rules;
        class SomeRule {
            private \PHPStan\FakeNs\Widget $scope;
            public function processNode() {
                $this->scope->getType();
            }
        }
        