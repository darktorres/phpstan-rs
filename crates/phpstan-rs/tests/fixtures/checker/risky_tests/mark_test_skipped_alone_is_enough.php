<?php
        namespace PHPUnit\Framework {
            class TestCase {}
        }
        namespace {
            
            class MyTest extends \PHPUnit\Framework\TestCase {
                public function testSomething() {
                    $this->markTestSkipped('not yet');
                }
            }
        
        }
        