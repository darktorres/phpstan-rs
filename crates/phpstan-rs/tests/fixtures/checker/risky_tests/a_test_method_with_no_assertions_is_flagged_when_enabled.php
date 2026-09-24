<?php
        namespace PHPUnit\Framework {
            class TestCase {}
        }
        namespace {
            
            class MyTest extends \PHPUnit\Framework\TestCase {
                public function testSomething() {
                    $x = 1 + 1;
                }
            }
        
        }
        