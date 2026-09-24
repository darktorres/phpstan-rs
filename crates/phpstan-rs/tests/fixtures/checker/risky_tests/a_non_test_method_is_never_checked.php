<?php
        namespace PHPUnit\Framework {
            class TestCase {}
        }
        namespace {
            
            class MyTest extends \PHPUnit\Framework\TestCase {
                public function helperMethod() {
                    $x = 1 + 1;
                }
            }
        
        }
        